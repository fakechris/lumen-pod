mod auth;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use auth::AppState;
use lumen_pod_core::store::Store;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

#[derive(Parser)]
#[command(name = "lumen-pod", version, about = "Agent-native podcast knowledge extraction")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Serve,
    Mcp,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lumen_pod_api=info,tower_http=info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://lumenpod:lumenpod@127.0.0.1:5488/lumenpod".into());

    match cli.command.unwrap_or(Commands::Serve) {
        Commands::Serve => run_server(&database_url).await,
        Commands::Mcp => run_mcp(&database_url).await,
    }
}

async fn run_mcp(database_url: &str) {
    if let Err(e) = lumen_pod_mcp::run_stdio(database_url).await {
        eprintln!("MCP error: {e}");
        std::process::exit(1);
    }
}

async fn run_server(database_url: &str) {
    let store = Store::new(database_url)
        .await
        .expect("failed to connect to database");
    tracing::info!(url = %database_url, "database connected");

    let auth_config = auth::AuthConfig::from_env();

    if std::env::var("RUST_ENV").unwrap_or_default() != "test" {
        auth_config.validate();
    }

    for email in &auth_config.admin_emails {
        if let Err(e) = store.seed_admin(email).await {
            tracing::warn!(%email, error = %e, "failed to seed admin");
        } else {
            tracing::info!(%email, "admin user seeded");
        }
    }

    let llm_config = LlmConfig::from_env();
    if llm_config.api_key.is_some() {
        tracing::info!(model = %llm_config.model, base = %llm_config.base_url, "LLM integration enabled");
    }

    // Ensure audio directory exists
    let audio_dir = std::env::var("AUDIO_DIR").unwrap_or_else(|_| "/data/audio".into());
    tokio::fs::create_dir_all(&audio_dir).await.ok();

    let state = AppState {
        store: Arc::new(store),
        auth: auth_config,
        llm: llm_config,
        audio_dir,
        batch_tasks: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };

    use axum::http::Method;
    let cors = match std::env::var("CORS_ORIGINS") {
        Ok(origins) => CorsLayer::new()
            .allow_origin(AllowOrigin::list(
                origins
                    .split(',')
                    .map(|o| o.trim().parse().expect("invalid CORS origin")),
            ))
            .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE, Method::OPTIONS])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT])
            .allow_credentials(true),
        Err(_) => CorsLayer::new()
            .allow_origin(AllowOrigin::any())
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any),
    };

    let public = Router::new()
        .route("/healthz", get(healthz))
        .route("/auth/google", get(auth::google_login))
        .route("/auth/google/callback", get(auth::google_callback))
        .route("/api/me", get(auth::me));

    // Serve audio files with Range support, MIME detection, streaming via ServeDir.
    // Public: filenames are UUID-based (unguessable). Authenticated access would
    // require signed URLs or token injection into <audio src>, not worth the complexity.
    let audio_serve = Router::new()
        .nest_service("/audio", tower_http::services::ServeDir::new(&state.audio_dir));

    let protected = Router::new()
        .route("/api/episodes", get(list_episodes).post(create_episode))
        .route("/api/episodes/submit-url", axum::routing::post(submit_url))
        .route(
            "/api/episodes/{id}",
            get(get_episode).patch(update_episode).delete(delete_episode),
        )
        .route(
            "/api/episodes/{id}/audio",
            axum::routing::post(upload_audio)
                .layer(axum::extract::DefaultBodyLimit::max(200 * 1024 * 1024)),
        )
        .route(
            "/api/episodes/{id}/chat",
            get(list_chat).post(create_chat_with_ai),
        )
        .route(
            "/api/episodes/{id}/chapters/{idx}/deep-read",
            axum::routing::post(chapter_deep_read),
        )
        .route(
            "/api/episodes/{id}/deep-read-all",
            axum::routing::post(batch_deep_read_start)
                .get(batch_deep_read_status)
                .delete(batch_deep_read_cancel),
        )
        .route("/api/episodes/{id}/export/{format}", get(export_episode))
        .route("/api/search", get(search_episodes))
        .route("/api/episodes/lookup", get(lookup_episode_by_url))
        .route("/api/stats", get(stats))
        .route(
            "/api/collections",
            get(list_collections).post(create_collection),
        )
        .route(
            "/api/invites",
            get(auth::list_invites).post(auth::create_invite),
        )
        .route("/api/admin/users", get(auth::list_users))
        .route(
            "/api/admin/users/{id}",
            axum::routing::patch(auth::update_user).delete(auth::delete_user),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    let app = public
        .merge(audio_serve)
        .merge(protected)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3210);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, "lumen-pod API server starting");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ---- Batch Deep Read Task ----

pub struct BatchDeepReadTask {
    cancelled: Arc<AtomicBool>,
    /// Index of the chapter currently being processed (usize::MAX = finished).
    current_idx: Arc<AtomicUsize>,
    total: usize,
    completed: Arc<AtomicUsize>,
    failed: Arc<AtomicUsize>,
    /// Epoch millis of last activity (chapter start or finish). Used for stale detection.
    last_heartbeat: Arc<AtomicU64>,
    started_at_ms: u64,
}

impl BatchDeepReadTask {
    fn is_stale(&self) -> bool {
        let now = epoch_ms();
        let hb = self.last_heartbeat.load(Ordering::Relaxed);
        // Stale if no heartbeat for 5 minutes
        now.saturating_sub(hb) > 5 * 60 * 1000
    }

    fn is_done(&self) -> bool {
        self.current_idx.load(Ordering::Relaxed) == usize::MAX
    }

    fn touch(&self) {
        self.last_heartbeat.store(epoch_ms(), Ordering::Relaxed);
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---- LLM Config (Anthropic Messages API) ----

#[derive(Clone)]
pub struct LlmConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
}

impl LlmConfig {
    fn from_env() -> Self {
        Self {
            api_key: std::env::var("LLM_API_KEY").ok(),
            base_url: std::env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "https://api.minimaxi.com/anthropic".into()),
            model: std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "MiniMax-M3.1-Flash-Preview".into()),
        }
    }
}

/// Call LLM using the Anthropic Messages API format
async fn call_llm_anthropic(
    config: &LlmConfig,
    system: &str,
    messages: &[serde_json::Value],
) -> String {
    let api_key = match &config.api_key {
        Some(k) => k,
        None => return "AI not configured. Set LLM_API_KEY to enable.".into(),
    };

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/messages", config.base_url))
        .header("x-api-key", api_key.as_str())
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": config.model,
            "max_tokens": 8192,
            "system": system,
            "messages": messages,
        }))
        .send()
        .await;

    match response {
        Ok(res) => {
            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                tracing::error!(%status, %body, "LLM API error");
                return format!("LLM API error: {status}. {body}");
            }
            match res.json::<serde_json::Value>().await {
                Ok(json) => {
                    // Anthropic response: {"content": [{"type": "text", "text": "..."}]}
                    json["content"]
                        .as_array()
                        .and_then(|arr| arr.iter().find(|c| c["type"] == "text"))
                        .and_then(|c| c["text"].as_str())
                        .unwrap_or("No response from AI")
                        .to_string()
                }
                Err(e) => format!("Failed to parse LLM response: {e}"),
            }
        }
        Err(e) => format!("Failed to call LLM: {e}"),
    }
}

// ---- Episode Handlers ----

#[derive(Deserialize)]
struct ListParams {
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_episodes(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);
    match state.store.list_episodes(limit, offset).await {
        Ok(episodes) => {
            let total = state.store.episode_count().await.unwrap_or(0);
            (
                StatusCode::OK,
                Json(serde_json::json!({ "total": total, "episodes": episodes })),
            )
                .into_response()
        }
        Err(e) => err_500(e),
    }
}

async fn get_episode(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_episode(&id).await {
        Ok(ep) => (StatusCode::OK, Json(serde_json::to_value(ep).unwrap())).into_response(),
        Err(lumen_pod_core::error::PodError::NotFound(_)) => err_404(),
        Err(e) => err_500(e),
    }
}

async fn create_episode(
    State(state): State<AppState>,
    Json(mut episode): Json<lumen_pod_core::episode::Episode>,
) -> impl IntoResponse {
    if episode.id.is_empty() {
        episode.id = uuid::Uuid::new_v4().to_string();
    }
    let now = chrono::Utc::now();
    episode.created_at = now;
    episode.updated_at = now;
    match state.store.insert_episode(&episode).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id": episode.id})),
        )
            .into_response(),
        Err(e) => err_500(e),
    }
}

async fn update_episode(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(updates): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state.store.update_episode(&id, &updates).await {
        Ok(ep) => (StatusCode::OK, Json(serde_json::to_value(ep).unwrap())).into_response(),
        Err(lumen_pod_core::error::PodError::NotFound(_)) => err_404(),
        Err(e) => err_500(e),
    }
}

async fn delete_episode(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.delete_episode(&id).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response(),
        Ok(false) => err_404(),
        Err(e) => err_500(e),
    }
}

// ---- Audio Upload ----

async fn upload_audio(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "empty body — send raw audio bytes"})),
        )
            .into_response();
    }

    // Verify episode exists
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };

    let ext = detect_audio_ext(&body);
    let filename = format!("{}.{}", id, ext);
    let filepath = std::path::Path::new(&state.audio_dir).join(&filename);

    // Remove any previous audio for this episode (may have different extension)
    if let Ok(mut entries) = tokio::fs::read_dir(&state.audio_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(&format!("{}.", id)) && *name_str != filename {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }

    if let Err(e) = tokio::fs::write(&filepath, &body).await {
        return err_500(format!("failed to write audio: {e}"));
    }

    let audio_url = format!("/audio/{}", filename);
    tracing::info!(%id, bytes = body.len(), ext, path = %filepath.display(), "audio uploaded");

    // Update source.audioPath
    let mut source_val = serde_json::to_value(&ep.source).unwrap_or_default();
    source_val["audioPath"] = serde_json::Value::String(audio_url.clone());
    let updates = serde_json::json!({"source": source_val});
    if let Err(e) = state.store.update_episode(&id, &updates).await {
        return err_500(e);
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "audioPath": audio_url,
            "format": ext,
            "sizeBytes": body.len()
        })),
    )
        .into_response()
}

fn detect_audio_ext(bytes: &[u8]) -> &'static str {
    if bytes.len() < 12 {
        return "mp3";
    }
    if bytes.starts_with(b"fLaC") {
        return "flac";
    }
    if bytes.starts_with(b"OggS") {
        return "ogg";
    }
    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        return "wav";
    }
    if bytes.starts_with(b"ID3") || (bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0) {
        return "mp3";
    }
    if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        return "m4a";
    }
    "mp3"
}

// ---- Submit URL ----

#[derive(Deserialize)]
struct SubmitUrlRequest {
    url: String,
    title: Option<String>,
}

async fn submit_url(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SubmitUrlRequest>,
) -> impl IntoResponse {
    if let Err(e) = auth::require_approved(&headers, &state.auth.jwt_secret) {
        return e;
    }

    let url = req.url.trim().to_string();
    if url.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "url is required"})),
        )
            .into_response();
    }

    if let Ok(true) = state.store.episode_exists_by_url(&url).await {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "episode with this URL already exists"})),
        )
            .into_response();
    }

    let platform = detect_platform(&url);
    let title = req.title.unwrap_or_else(|| format!("Pending: {}", &url));

    let episode = lumen_pod_core::episode::Episode {
        id: uuid::Uuid::new_v4().to_string(),
        source: lumen_pod_core::episode::Source {
            url: url.clone(),
            platform,
            title,
            author: String::new(),
            published_at: None,
            thumbnail_url: None,
            duration: 0.0,
            feed_url: None,
            description: None,
            podcast_name: None,
            episode_cover_url: None,
            language: None,
            audio_path: None,
        },
        processing: lumen_pod_core::episode::Processing {
            status: lumen_pod_core::episode::ProcessingStatus::Pending,
            asr_engine: None,
            llm_model: None,
            processed_at: None,
            agent_id: None,
            error: None,
        },
        summary: None,
        chapters: vec![],
        mindmap: None,
        transcript: None,
        keywords: vec![],
        highlights: vec![],
        qas: vec![],
        clips: vec![],
        article: None,
        shownotes: None,
        is_read: false,
        tags: vec!["pending".to_string()],
        collection_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    match state.store.insert_episode(&episode).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": episode.id,
                "url": url,
                "status": "pending",
                "message": "Episode submitted. Use MCP agent or skill to process it."
            })),
        )
            .into_response(),
        Err(e) => err_500(e),
    }
}

fn detect_platform(url: &str) -> String {
    if url.contains("youtube.com") || url.contains("youtu.be") {
        "youtube".into()
    } else if url.contains("bilibili.com") || url.contains("b23.tv") {
        "bilibili".into()
    } else if url.contains("spotify.com") {
        "spotify".into()
    } else if url.contains("apple.com/podcast") || url.contains("podcasts.apple.com") {
        "apple".into()
    } else if url.contains("twitter.com") || url.contains("x.com") {
        "twitter".into()
    } else if url.contains("xiaoyuzhou") || url.contains("xyzfm") {
        "xiaoyuzhou".into()
    } else {
        "other".into()
    }
}

// ---- Lookup by URL ----

#[derive(Deserialize)]
struct LookupParams {
    url: String,
}

async fn lookup_episode_by_url(
    State(state): State<AppState>,
    Query(params): Query<LookupParams>,
) -> impl IntoResponse {
    let url = params.url.trim().to_string();
    if url.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "url parameter required"}))).into_response();
    }

    // Try exact URL match first
    match state.store.find_episode_by_url(&url).await {
        Ok(Some(ep)) => (StatusCode::OK, Json(serde_json::to_value(ep).unwrap())).into_response(),
        Ok(None) => {
            // For YouTube, also try alternate URL forms
            if let Some(video_id) = extract_youtube_video_id(&url) {
                let alt_urls = [
                    format!("https://www.youtube.com/watch?v={video_id}"),
                    format!("https://youtube.com/watch?v={video_id}"),
                    format!("https://youtu.be/{video_id}"),
                ];
                for alt in &alt_urls {
                    if alt == &url { continue; }
                    if let Ok(Some(ep)) = state.store.find_episode_by_url(alt).await {
                        return (StatusCode::OK, Json(serde_json::to_value(ep).unwrap())).into_response();
                    }
                }
            }
            (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))).into_response()
        }
        Err(e) => err_500(e),
    }
}

fn extract_youtube_video_id(url: &str) -> Option<String> {
    if url.contains("youtu.be/") {
        let after = url.split("youtu.be/").nth(1)?;
        let id = after.split(['?', '&', '#']).next()?;
        if !id.is_empty() { return Some(id.to_string()); }
    }
    if url.contains("youtube.com/watch") || url.contains("youtube.com/embed/") {
        if url.contains("v=") {
            let after = url.split("v=").nth(1)?;
            let id = after.split(['&', '#']).next()?;
            if !id.is_empty() { return Some(id.to_string()); }
        }
        if url.contains("/embed/") {
            let after = url.split("/embed/").nth(1)?;
            let id = after.split(['?', '&', '#']).next()?;
            if !id.is_empty() { return Some(id.to_string()); }
        }
    }
    None
}

// ---- Search ----

#[derive(Deserialize)]
struct SearchParams {
    q: String,
    limit: Option<usize>,
}

async fn search_episodes(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(10).min(50);
    match state.store.search_episodes(&params.q, limit).await {
        Ok(ids) => {
            let mut episodes = Vec::new();
            for id in &ids {
                if let Ok(ep) = state.store.get_episode(id).await {
                    episodes.push(ep);
                }
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"results": episodes})),
            )
                .into_response()
        }
        Err(e) => err_500(e),
    }
}

// ---- Chat with AI ----

#[derive(Deserialize)]
struct ChatRequest {
    #[allow(dead_code)]
    role: String,
    content: String,
}

async fn list_chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.list_chat_messages(&id).await {
        Ok(messages) => (StatusCode::OK, Json(serde_json::json!({"messages": messages}))).into_response(),
        Err(e) => err_500(e),
    }
}

async fn create_chat_with_ai(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let user_msg = match state.store.add_chat_message(&id, "user", &req.content).await {
        Ok(msg) => msg,
        Err(e) => return err_500(e),
    };

    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(_) => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "userMessage": serde_json::to_value(&user_msg).unwrap(),
                    "assistantMessage": {
                        "id": uuid::Uuid::new_v4().to_string(),
                        "episodeId": id,
                        "role": "assistant",
                        "content": "Episode not found.",
                        "createdAt": chrono::Utc::now()
                    }
                })),
            )
                .into_response();
        }
    };

    let history = state.store.list_chat_messages(&id).await.unwrap_or_default();

    // Build context
    let system = build_chat_system_prompt(&ep);

    // Build message history for Anthropic format
    let mut messages = Vec::new();
    for msg in history.iter().rev().take(20).collect::<Vec<_>>().into_iter().rev() {
        let role = match msg.role {
            lumen_pod_core::episode::ChatRole::User => "user",
            lumen_pod_core::episode::ChatRole::Assistant => "assistant",
        };
        messages.push(serde_json::json!({"role": role, "content": msg.content}));
    }
    messages.push(serde_json::json!({"role": "user", "content": req.content}));

    let assistant_content = call_llm_anthropic(&state.llm, &system, &messages).await;

    let assistant_msg = match state.store.add_chat_message(&id, "assistant", &assistant_content).await {
        Ok(msg) => msg,
        Err(e) => return err_500(e),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "userMessage": serde_json::to_value(&user_msg).unwrap(),
            "assistantMessage": serde_json::to_value(&assistant_msg).unwrap()
        })),
    )
        .into_response()
}

fn build_chat_system_prompt(ep: &lumen_pod_core::episode::Episode) -> String {
    let mut context = format!("Episode: {}\nAuthor: {}\n", ep.source.title, ep.source.author);

    if let Some(s) = &ep.summary {
        context.push_str(&format!("\nSummary: {}\n{}\n", s.one_liner, s.r#abstract));
        if !s.key_takeaways.is_empty() {
            context.push_str("Key Takeaways:\n");
            for t in &s.key_takeaways {
                context.push_str(&format!("- {t}\n"));
            }
        }
    }

    if !ep.chapters.is_empty() {
        context.push_str("\nChapters:\n");
        for ch in &ep.chapters {
            context.push_str(&format!("- [{}] {} — {}\n", fmt_dur(ch.start_time), ch.title, ch.summary));
        }
    }

    if let Some(t) = &ep.transcript {
        context.push_str("\nTranscript excerpt (first 3000 chars):\n");
        let mut transcript_text = String::new();
        for para in &t.paragraphs {
            if let Some(speaker) = &para.speaker {
                transcript_text.push_str(&format!("[{speaker}] "));
            }
            for sent in &para.sentences {
                transcript_text.push_str(&sent.text);
                transcript_text.push(' ');
            }
            transcript_text.push('\n');
            if transcript_text.len() > 3000 { break; }
        }
        context.push_str(&transcript_text);
    }

    format!(
        "You are an AI assistant helping users understand a podcast/video episode. \
         Answer questions based on the episode content below. Be concise but thorough. \
         If the answer isn't in the content, say so.\n\n---\n{context}"
    )
}

// ---- Chapter Deep Read ----

#[derive(Deserialize)]
struct DeepReadParams {
    lang: Option<String>,
    regenerate: Option<bool>,
}

fn deep_read_system_prompt(lang: &str) -> String {
    let lang_instruction = match lang {
        "zh" => "You MUST write the entire response in Chinese (简体中文). All headings, analysis, and text must be in Chinese.",
        "en" => "Write the entire response in English.",
        "ja" => "日本語で書いてください。見出し、分析、テキストのすべてを日本語で記述してください。",
        "ko" => "전체 응답을 한국어로 작성하세요.",
        "es" => "Escribe toda la respuesta en español.",
        "fr" => "Rédigez l'intégralité de la réponse en français.",
        "de" => "Schreiben Sie die gesamte Antwort auf Deutsch.",
        other => return format!(
            "You are an expert content analyst. Write detailed, polished deep-read articles about podcast/video chapters. \
             Include key insights, context, and analysis. Write in a clear, engaging style. \
             You MUST write the entire response in {other}."
        ),
    };
    format!(
        "You are an expert content analyst. Write detailed, polished deep-read articles about podcast/video chapters. \
         Include key insights, context, and analysis. Write in a clear, engaging style.\n\n{lang_instruction}"
    )
}

async fn chapter_deep_read(
    State(state): State<AppState>,
    Path((id, idx)): Path<(String, usize)>,
    Query(params): Query<DeepReadParams>,
) -> impl IntoResponse {
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };

    if idx >= ep.chapters.len() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "chapter index out of range"})),
        )
            .into_response();
    }

    let chapter = &ep.chapters[idx];
    let regenerate = params.regenerate.unwrap_or(false);

    // Return cached deep read if exists and not regenerating
    if !regenerate {
        if let Some(cached) = &chapter.deep_read {
            return (StatusCode::OK, Json(serde_json::json!({"content": cached, "cached": true}))).into_response();
        }
    }

    let lang = params.lang.as_deref().unwrap_or("zh");
    let transcript_excerpt = build_chapter_transcript(&ep, chapter);
    let system = deep_read_system_prompt(lang);

    let prompt = format!(
        "Write a deep-read analysis of this chapter:\n\n\
         Chapter: {}\nTime: {} — {}\nOriginal Summary: {}\n\n\
         Transcript:\n{}",
        chapter.title,
        fmt_dur(chapter.start_time),
        fmt_dur(chapter.end_time),
        chapter.summary,
        &transcript_excerpt[..transcript_excerpt.len().min(4000)]
    );

    let messages = vec![serde_json::json!({"role": "user", "content": prompt})];
    let content = call_llm_anthropic(&state.llm, &system, &messages).await;

    // Persist deep read into the chapter
    let mut chapters_val = serde_json::to_value(&ep.chapters).unwrap_or_default();
    if let Some(arr) = chapters_val.as_array_mut() {
        if let Some(ch) = arr.get_mut(idx) {
            ch["deepRead"] = serde_json::Value::String(content.clone());
        }
    }
    let updates = serde_json::json!({"chapters": chapters_val});
    if let Err(e) = state.store.update_episode(&id, &updates).await {
        tracing::warn!(%id, idx, error = %e, "failed to persist deep read");
    }

    (StatusCode::OK, Json(serde_json::json!({"content": content, "cached": false}))).into_response()
}

// ---- Batch Deep Read ----

#[derive(Deserialize)]
struct BatchDeepReadParams {
    lang: Option<String>,
}

async fn batch_deep_read_start(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<BatchDeepReadParams>,
) -> impl IntoResponse {
    let lang = params.lang.unwrap_or_else(|| "zh".into());
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };

    if ep.chapters.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "no chapters"}))).into_response();
    }

    // Chapters that still need deep reads
    let pending: Vec<usize> = ep.chapters.iter().enumerate()
        .filter(|(_, ch)| ch.deep_read.is_none())
        .map(|(i, _)| i)
        .collect();

    if pending.is_empty() {
        return (StatusCode::OK, Json(serde_json::json!({
            "status": "complete", "message": "all chapters already have deep reads",
            "total": ep.chapters.len(), "completed": ep.chapters.len(), "pending": 0
        }))).into_response();
    }

    // Acquire lock — one batch per episode
    {
        let mut tasks = state.batch_tasks.lock().unwrap();
        if let Some(existing) = tasks.get(&id) {
            if !existing.is_done() && !existing.is_stale() {
                return (StatusCode::CONFLICT, Json(serde_json::json!({
                    "error": "batch already running",
                    "currentIdx": existing.current_idx.load(Ordering::Relaxed),
                    "completed": existing.completed.load(Ordering::Relaxed),
                    "total": existing.total,
                }))).into_response();
            }
            // Stale or done — remove and proceed
        }

        let now = epoch_ms();
        let task = BatchDeepReadTask {
            cancelled: Arc::new(AtomicBool::new(false)),
            current_idx: Arc::new(AtomicUsize::new(pending[0])),
            total: pending.len(),
            completed: Arc::new(AtomicUsize::new(0)),
            failed: Arc::new(AtomicUsize::new(0)),
            last_heartbeat: Arc::new(AtomicU64::new(now)),
            started_at_ms: now,
        };
        tasks.insert(id.clone(), task);
    }

    // Clone what the background task needs
    let task_state = state.clone();
    let task_id = id.clone();
    let task_pending = pending.clone();
    let task_lang = lang.clone();

    tokio::spawn(async move {
        batch_deep_read_worker(task_state, task_id, task_pending, &task_lang).await;
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({
        "status": "started",
        "pending": pending.len(),
        "chapters": pending,
    }))).into_response()
}

async fn batch_deep_read_worker(state: AppState, episode_id: String, pending: Vec<usize>, lang: &str) {
    let system = deep_read_system_prompt(lang);
    for &idx in &pending {
        // Check cancellation
        let cancelled = {
            let tasks = state.batch_tasks.lock().unwrap();
            match tasks.get(&episode_id) {
                Some(t) => {
                    t.current_idx.store(idx, Ordering::Relaxed);
                    t.touch();
                    t.cancelled.load(Ordering::Relaxed)
                }
                None => true, // task removed externally
            }
        };
        if cancelled { break; }

        // Re-fetch episode each iteration to get latest state
        let ep = match state.store.get_episode(&episode_id).await {
            Ok(ep) => ep,
            Err(_) => break,
        };

        // Skip if deep read already exists (race with single-chapter request)
        if idx < ep.chapters.len() && ep.chapters[idx].deep_read.is_some() {
            let tasks = state.batch_tasks.lock().unwrap();
            if let Some(t) = tasks.get(&episode_id) {
                t.completed.fetch_add(1, Ordering::Relaxed);
                t.touch();
            }
            continue;
        }

        if idx >= ep.chapters.len() { continue; }

        let chapter = &ep.chapters[idx];
        let transcript_excerpt = build_chapter_transcript(&ep, chapter);

        let prompt = format!(
            "Write a deep-read analysis of this chapter:\n\n\
             Chapter: {}\nTime: {} — {}\nOriginal Summary: {}\n\n\
             Transcript:\n{}",
            chapter.title,
            fmt_dur(chapter.start_time),
            fmt_dur(chapter.end_time),
            chapter.summary,
            &transcript_excerpt[..transcript_excerpt.len().min(4000)]
        );

        let messages = vec![serde_json::json!({"role": "user", "content": prompt})];

        // Timeout: 3 minutes per chapter
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(180),
            call_llm_anthropic(&state.llm, &system, &messages),
        ).await;

        let is_ok = match result {
            Ok(content) if !content.starts_with("LLM API error") && !content.starts_with("Failed to") => {
                // Persist
                let mut chapters_val = serde_json::to_value(&ep.chapters).unwrap_or_default();
                if let Some(arr) = chapters_val.as_array_mut() {
                    if let Some(ch) = arr.get_mut(idx) {
                        ch["deepRead"] = serde_json::Value::String(content);
                    }
                }
                let updates = serde_json::json!({"chapters": chapters_val});
                state.store.update_episode(&episode_id, &updates).await.is_ok()
            }
            Ok(err_content) => {
                tracing::warn!(%episode_id, idx, %err_content, "batch deep read LLM error");
                false
            }
            Err(_timeout) => {
                tracing::warn!(%episode_id, idx, "batch deep read timed out (180s)");
                false
            }
        };

        {
            let tasks = state.batch_tasks.lock().unwrap();
            if let Some(t) = tasks.get(&episode_id) {
                if is_ok {
                    t.completed.fetch_add(1, Ordering::Relaxed);
                } else {
                    t.failed.fetch_add(1, Ordering::Relaxed);
                }
                t.touch();
            }
        }
    }

    // Mark done and schedule cleanup
    {
        let tasks = state.batch_tasks.lock().unwrap();
        if let Some(t) = tasks.get(&episode_id) {
            t.current_idx.store(usize::MAX, Ordering::Relaxed);
            t.touch();
        }
    }

    // Remove task after 60 seconds so frontend can read final status
    let cleanup_state = state.clone();
    let cleanup_id = episode_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        cleanup_state.batch_tasks.lock().unwrap().remove(&cleanup_id);
    });
}

fn build_chapter_transcript(ep: &lumen_pod_core::episode::Episode, chapter: &lumen_pod_core::episode::Chapter) -> String {
    let Some(t) = &ep.transcript else { return String::new() };
    let mut text = String::new();
    for para in &t.paragraphs {
        let para_start = para.sentences.first().and_then(|s| s.words.first().map(|w| w.start)).unwrap_or(0.0);
        let para_end = para.sentences.last().and_then(|s| s.words.last().map(|w| w.end)).unwrap_or(0.0);
        if para_end >= chapter.start_time && para_start <= chapter.end_time {
            if let Some(speaker) = &para.speaker {
                text.push_str(&format!("[{speaker}] "));
            }
            for sent in &para.sentences {
                text.push_str(&sent.text);
                text.push(' ');
            }
            text.push('\n');
        }
    }
    text
}

async fn batch_deep_read_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tasks = state.batch_tasks.lock().unwrap();
    match tasks.get(&id) {
        Some(t) => {
            let current = t.current_idx.load(Ordering::Relaxed);
            let status = if t.cancelled.load(Ordering::Relaxed) {
                "cancelled"
            } else if current == usize::MAX {
                "complete"
            } else {
                "running"
            };
            (StatusCode::OK, Json(serde_json::json!({
                "status": status,
                "currentIdx": if current == usize::MAX { serde_json::Value::Null } else { serde_json::json!(current) },
                "total": t.total,
                "completed": t.completed.load(Ordering::Relaxed),
                "failed": t.failed.load(Ordering::Relaxed),
                "elapsedMs": epoch_ms().saturating_sub(t.started_at_ms),
            }))).into_response()
        }
        None => (StatusCode::OK, Json(serde_json::json!({"status": "idle"}))).into_response(),
    }
}

async fn batch_deep_read_cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tasks = state.batch_tasks.lock().unwrap();
    match tasks.get(&id) {
        Some(t) if !t.is_done() => {
            t.cancelled.store(true, Ordering::Relaxed);
            (StatusCode::OK, Json(serde_json::json!({
                "status": "cancelling",
                "completed": t.completed.load(Ordering::Relaxed),
            }))).into_response()
        }
        _ => (StatusCode::OK, Json(serde_json::json!({"status": "idle"}))).into_response(),
    }
}

// ---- Export ----

async fn export_episode(
    State(state): State<AppState>,
    Path((id, format)): Path<(String, String)>,
) -> impl IntoResponse {
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };

    match format.as_str() {
        "markdown" | "md" => {
            let md = build_markdown(&ep);
            (StatusCode::OK, [(header::CONTENT_TYPE, "text/markdown; charset=utf-8"), (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{}.md\"", sanitize_filename(&ep.source.title)))], md).into_response()
        }
        "srt" => {
            let srt = build_srt(&ep);
            (StatusCode::OK, [(header::CONTENT_TYPE, "application/x-subrip; charset=utf-8"), (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{}.srt\"", sanitize_filename(&ep.source.title)))], srt).into_response()
        }
        "json" => {
            let json = serde_json::to_string_pretty(&ep).unwrap_or_default();
            (StatusCode::OK, [(header::CONTENT_TYPE, "application/json; charset=utf-8"), (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{}.json\"", sanitize_filename(&ep.source.title)))], json).into_response()
        }
        "notion" => {
            let md = build_notion_export(&ep);
            (StatusCode::OK, [(header::CONTENT_TYPE, "text/markdown; charset=utf-8"), (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{} (Notion).md\"", sanitize_filename(&ep.source.title)))], md).into_response()
        }
        "obsidian" => {
            let md = build_obsidian_export(&ep);
            (StatusCode::OK, [(header::CONTENT_TYPE, "text/markdown; charset=utf-8"), (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{}.md\"", sanitize_filename(&ep.source.title)))], md).into_response()
        }
        _ => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "unsupported format. use: markdown, srt, json, notion, obsidian"}))).into_response(),
    }
}

fn build_markdown(ep: &lumen_pod_core::episode::Episode) -> String {
    use std::fmt::Write;
    let mut md = String::new();
    writeln!(md, "# {}\n", ep.source.title).ok();
    writeln!(md, "**Author:** {}  ", ep.source.author).ok();
    writeln!(md, "**Duration:** {}  ", fmt_dur(ep.source.duration)).ok();
    writeln!(md, "**Source:** [{}]({})\n", ep.source.platform, ep.source.url).ok();

    if let Some(s) = &ep.summary {
        writeln!(md, "## Summary\n> {}\n\n{}\n", s.one_liner, s.r#abstract).ok();
        if !s.key_takeaways.is_empty() {
            writeln!(md, "### Key Takeaways\n").ok();
            for t in &s.key_takeaways { writeln!(md, "- {t}").ok(); }
            writeln!(md).ok();
        }
        if !s.glossary.is_empty() {
            writeln!(md, "### Glossary\n").ok();
            for g in &s.glossary { writeln!(md, "- **{}**: {}", g.term, g.definition).ok(); }
            writeln!(md).ok();
        }
    }

    if !ep.chapters.is_empty() {
        writeln!(md, "## Chapters\n").ok();
        for ch in &ep.chapters { writeln!(md, "- **[{}]** {} — {}", fmt_dur(ch.start_time), ch.title, ch.summary).ok(); }
        writeln!(md).ok();
    }

    if !ep.keywords.is_empty() {
        writeln!(md, "## Keywords\n").ok();
        for kw in &ep.keywords {
            match &kw.desc {
                Some(desc) => writeln!(md, "- **{}**: {desc}", kw.key).ok(),
                None => writeln!(md, "- **{}**", kw.key).ok(),
            };
        }
        writeln!(md).ok();
    }

    if !ep.qas.is_empty() {
        writeln!(md, "## Q&A\n").ok();
        for qa in &ep.qas { writeln!(md, "**Q: {}**\n\nA: {}\n", qa.question, qa.answer).ok(); }
    }

    if let Some(t) = &ep.transcript {
        writeln!(md, "## Transcript\n").ok();
        for para in &t.paragraphs {
            if let Some(speaker) = &para.speaker { write!(md, "**{speaker}:** ").ok(); }
            let text: String = para.sentences.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
            writeln!(md, "{text}\n").ok();
        }
    }

    md
}

fn build_srt(ep: &lumen_pod_core::episode::Episode) -> String {
    use std::fmt::Write;
    let mut srt = String::new();
    let mut idx = 1u32;
    if let Some(t) = &ep.transcript {
        for para in &t.paragraphs {
            for sent in &para.sentences {
                if sent.words.is_empty() { continue; }
                let start = sent.words.first().map(|w| w.start).unwrap_or(0.0);
                let end = sent.words.last().map(|w| w.end).unwrap_or(0.0);
                writeln!(srt, "{idx}").ok();
                writeln!(srt, "{} --> {}", srt_time(start), srt_time(end)).ok();
                if let Some(speaker) = &para.speaker {
                    writeln!(srt, "[{speaker}] {}", sent.text).ok();
                } else {
                    writeln!(srt, "{}", sent.text).ok();
                }
                writeln!(srt).ok();
                idx += 1;
            }
        }
    }
    srt
}

fn build_notion_export(ep: &lumen_pod_core::episode::Episode) -> String {
    use std::fmt::Write;
    let mut md = String::new();
    writeln!(md, "# {}\n", ep.source.title).ok();
    writeln!(md, "| Property | Value |\n|---|---|").ok();
    writeln!(md, "| Source | [{}]({}) |", ep.source.platform, ep.source.url).ok();
    writeln!(md, "| Author | {} |", ep.source.author).ok();
    writeln!(md, "| Duration | {} |", fmt_dur(ep.source.duration)).ok();
    if let Some(lang) = &ep.source.language { writeln!(md, "| Language | {} |", lang).ok(); }
    if let Some(pub_at) = &ep.source.published_at { writeln!(md, "| Published | {} |", pub_at.format("%Y-%m-%d")).ok(); }
    writeln!(md, "| Tags | {} |\n", ep.tags.join(", ")).ok();

    if let Some(s) = &ep.summary {
        writeln!(md, "## TL;DR\n> {}\n\n{}\n", s.one_liner, s.r#abstract).ok();
        if !s.key_takeaways.is_empty() {
            writeln!(md, "## Key Takeaways\n").ok();
            for (i, t) in s.key_takeaways.iter().enumerate() { writeln!(md, "{}. {t}", i + 1).ok(); }
            writeln!(md).ok();
        }
    }
    if !ep.chapters.is_empty() {
        writeln!(md, "## Chapters\n").ok();
        for ch in &ep.chapters { writeln!(md, "### {} ({})\n{}\n", ch.title, fmt_dur(ch.start_time), ch.summary).ok(); }
    }
    if let Some(mm) = &ep.mindmap { writeln!(md, "## Mind Map\n```\n{}\n```\n", mm.markdown).ok(); }
    if !ep.highlights.is_empty() {
        writeln!(md, "## Highlights\n").ok();
        for h in &ep.highlights { writeln!(md, "- **[{}]** {}", fmt_dur(h.time), h.content).ok(); }
        writeln!(md).ok();
    }
    if !ep.keywords.is_empty() {
        writeln!(md, "## Keywords\n").ok();
        for kw in &ep.keywords {
            match &kw.desc { Some(d) => writeln!(md, "- **{}** — {d}", kw.key).ok(), None => writeln!(md, "- {}", kw.key).ok() };
        }
        writeln!(md).ok();
    }
    if !ep.qas.is_empty() {
        writeln!(md, "## Q&A\n").ok();
        for qa in &ep.qas { writeln!(md, "<details><summary>{}</summary>\n{}\n</details>\n", qa.question, qa.answer).ok(); }
    }
    md
}

fn build_obsidian_export(ep: &lumen_pod_core::episode::Episode) -> String {
    use std::fmt::Write;
    let mut md = String::new();
    writeln!(md, "---\ntitle: \"{}\"", ep.source.title.replace('"', "'")).ok();
    writeln!(md, "source: {}\nplatform: {}", ep.source.url, ep.source.platform).ok();
    writeln!(md, "author: \"{}\"", ep.source.author.replace('"', "'")).ok();
    writeln!(md, "duration: {}", fmt_dur(ep.source.duration)).ok();
    if let Some(lang) = &ep.source.language { writeln!(md, "language: {lang}").ok(); }
    if let Some(pub_at) = &ep.source.published_at { writeln!(md, "published: {}", pub_at.format("%Y-%m-%d")).ok(); }
    if !ep.tags.is_empty() { writeln!(md, "tags:").ok(); for tag in &ep.tags { writeln!(md, "  - {tag}").ok(); } }
    writeln!(md, "created: {}\n---\n", ep.created_at.format("%Y-%m-%dT%H:%M:%S")).ok();

    writeln!(md, "# {}\n", ep.source.title).ok();

    if let Some(s) = &ep.summary {
        writeln!(md, "## Summary\n> [!tip] TL;DR\n> {}\n\n{}\n", s.one_liner, s.r#abstract).ok();
        if !s.key_takeaways.is_empty() {
            writeln!(md, "### Key Takeaways\n").ok();
            for t in &s.key_takeaways { writeln!(md, "- {t}").ok(); }
            writeln!(md).ok();
        }
        if !s.glossary.is_empty() {
            writeln!(md, "### Glossary\n").ok();
            for g in &s.glossary { writeln!(md, "- **{}**:: {}", g.term, g.definition).ok(); }
            writeln!(md).ok();
        }
        if !s.thinking_questions.is_empty() {
            writeln!(md, "### Thinking Questions\n").ok();
            for q in &s.thinking_questions { writeln!(md, "- [ ] {q}").ok(); }
            writeln!(md).ok();
        }
    }
    if !ep.chapters.is_empty() {
        writeln!(md, "## Chapters\n").ok();
        for ch in &ep.chapters { writeln!(md, "### {} `{}`\n{}\n", ch.title, fmt_dur(ch.start_time), ch.summary).ok(); }
    }
    if let Some(mm) = &ep.mindmap { writeln!(md, "## Mind Map\n```markmap\n{}\n```\n", mm.markdown).ok(); }
    if !ep.highlights.is_empty() {
        writeln!(md, "## Highlights\n").ok();
        for h in &ep.highlights { writeln!(md, "- =={} ({})== {}", h.content, fmt_dur(h.time), h.note.as_deref().unwrap_or("")).ok(); }
        writeln!(md).ok();
    }
    if !ep.keywords.is_empty() {
        writeln!(md, "## Keywords\n{}\n", ep.keywords.iter().map(|k| format!("[[{}]]", k.key)).collect::<Vec<_>>().join(" · ")).ok();
    }
    if !ep.qas.is_empty() {
        writeln!(md, "## Q&A\n").ok();
        for qa in &ep.qas { writeln!(md, "> [!question] {}\n> {}\n", qa.question, qa.answer).ok(); }
    }
    if let Some(t) = &ep.transcript {
        writeln!(md, "## Transcript\n").ok();
        for para in &t.paragraphs {
            if let Some(speaker) = &para.speaker { write!(md, "**{speaker}:** ").ok(); }
            let text: String = para.sentences.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
            writeln!(md, "{text}\n").ok();
        }
    }
    md
}

fn srt_time(seconds: f64) -> String {
    let h = (seconds / 3600.0) as u32;
    let m = ((seconds % 3600.0) / 60.0) as u32;
    let s = (seconds % 60.0) as u32;
    let ms = ((seconds % 1.0) * 1000.0) as u32;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

fn sanitize_filename(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect::<String>().trim().to_string()
}

fn fmt_dur(seconds: f64) -> String {
    let h = (seconds / 3600.0) as u32;
    let m = ((seconds % 3600.0) / 60.0) as u32;
    let s = (seconds % 60.0) as u32;
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

// ---- Collections ----

#[derive(Deserialize)]
struct CreateCollectionRequest {
    name: String,
    description: Option<String>,
}

async fn list_collections(State(state): State<AppState>) -> impl IntoResponse {
    match state.store.list_collections().await {
        Ok(cols) => (StatusCode::OK, Json(serde_json::json!({"collections": cols}))).into_response(),
        Err(e) => err_500(e),
    }
}

async fn create_collection(
    State(state): State<AppState>,
    Json(req): Json<CreateCollectionRequest>,
) -> impl IntoResponse {
    match state.store.create_collection(&req.name, req.description.as_deref()).await {
        Ok(col) => (StatusCode::CREATED, Json(serde_json::to_value(col).unwrap())).into_response(),
        Err(e) => err_500(e),
    }
}

// ---- Utils ----

async fn stats(State(state): State<AppState>) -> impl IntoResponse {
    let count = state.store.episode_count().await.unwrap_or(0);
    Json(serde_json::json!({"episode_count": count}))
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

fn err_404() -> axum::response::Response {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))).into_response()
}

fn err_500(e: impl std::fmt::Display) -> axum::response::Response {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
}
