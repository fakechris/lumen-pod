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
        article_tasks: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
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

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "/data/static".into());
    tokio::fs::create_dir_all(&static_dir).await.ok();
    let static_serve = Router::new()
        .nest_service("/static", tower_http::services::ServeDir::new(&static_dir));

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
            "/api/episodes/{id}/article",
            axum::routing::post(episode_article).get(episode_article_status),
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
        .route(
            "/api/admin/invites",
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
        .merge(static_serve)
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

/// Parse an Anthropic Messages API JSON payload and extract the text response.
/// Logs `stop_reason`, `input_tokens`, `output_tokens`, and `thinking_tokens`.
/// Returns `Err` if no non-empty `type == "text"` block is present (e.g. when a
/// reasoning model exhausts `max_tokens` during the `thinking` phase).
fn extract_llm_text_from_response(json: &serde_json::Value) -> Result<String, String> {
    let stop_reason = json["stop_reason"].as_str().unwrap_or("unknown");
    let input_tokens = json["usage"]["input_tokens"].as_u64().unwrap_or(0);
    let output_tokens = json["usage"]["output_tokens"].as_u64().unwrap_or(0);
    let thinking_tokens = json["usage"]["output_tokens_details"]["thinking_tokens"]
        .as_u64()
        .unwrap_or(0);

    let text = json["content"]
        .as_array()
        .and_then(|arr| {
            let joined: String = arr
                .iter()
                .filter(|c| c["type"] == "text")
                .filter_map(|c| c["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        });

    tracing::info!(
        %stop_reason,
        input_tokens,
        output_tokens,
        thinking_tokens,
        has_text = text.is_some(),
        "LLM response received"
    );

    match text {
        Some(t) => Ok(t),
        None => {
            tracing::warn!(
                %stop_reason,
                input_tokens,
                output_tokens,
                thinking_tokens,
                "LLM returned no text block (likely exhausted max_tokens during thinking)"
            );
            Err(format!(
                "LLM returned empty text response (stop_reason={stop_reason}, output_tokens={output_tokens}, thinking_tokens={thinking_tokens})"
            ))
        }
    }
}

/// Parse an Anthropic Messages SSE stream (`stream: true`) or non-streaming JSON body
/// and extract the text response.
/// Using `stream: true` prevents reverse-proxy / CDN idle read timeouts during 30-90s
/// reasoning (`thinking`) and long-form generation phases.
fn extract_llm_text_from_sse_or_json(body: &str) -> Result<String, String> {
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') {
        let json: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("Failed to parse LLM JSON response: {e}"))?;
        return extract_llm_text_from_response(&json);
    }

    let mut text_buf = String::new();
    let mut stop_reason = String::from("unknown");
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut thinking_tokens: u64 = 0;

    for line in body.lines() {
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        match ev["type"].as_str().unwrap_or("") {
            "message_start" => {
                if let Some(in_tok) = ev["message"]["usage"]["input_tokens"].as_u64() {
                    input_tokens = in_tok;
                }
            }
            "content_block_start" => {
                if ev["content_block"]["type"].as_str() == Some("text") {
                    if let Some(t) = ev["content_block"]["text"].as_str() {
                        text_buf.push_str(t);
                    }
                }
            }
            "content_block_delta" => {
                if ev["delta"]["type"].as_str() == Some("text_delta") {
                    if let Some(t) = ev["delta"]["text"].as_str() {
                        text_buf.push_str(t);
                    }
                }
            }
            "message_delta" => {
                if let Some(sr) = ev["delta"]["stop_reason"].as_str() {
                    stop_reason = sr.to_string();
                }
                if let Some(in_tok) = ev["usage"]["input_tokens"].as_u64() {
                    input_tokens = in_tok;
                }
                if let Some(out_tok) = ev["usage"]["output_tokens"].as_u64() {
                    output_tokens = out_tok;
                }
                if let Some(think_tok) =
                    ev["usage"]["output_tokens_details"]["thinking_tokens"].as_u64()
                {
                    thinking_tokens = think_tok;
                }
            }
            "error" => {
                let err_msg = ev["error"]["message"]
                    .as_str()
                    .unwrap_or("unknown stream error");
                return Err(format!("LLM stream error: {err_msg}"));
            }
            _ => {}
        }
    }

    let final_text = text_buf.trim().to_string();
    let has_text = !final_text.is_empty();

    tracing::info!(
        %stop_reason,
        input_tokens,
        output_tokens,
        thinking_tokens,
        has_text,
        text_chars = final_text.chars().count(),
        "LLM stream response received"
    );

    if has_text {
        Ok(final_text)
    } else {
        tracing::warn!(
            %stop_reason,
            input_tokens,
            output_tokens,
            thinking_tokens,
            "LLM stream returned no text block (likely exhausted max_tokens during thinking)"
        );
        Err(format!(
            "LLM returned empty text response (stop_reason={stop_reason}, output_tokens={output_tokens}, thinking_tokens={thinking_tokens})"
        ))
    }
}

const ARTICLE_TASK_TIMEOUT_MS: u64 = 2_400_000; // 40 minutes for 2-4 hour long-form episodes
const LLM_STREAM_TOTAL_TIMEOUT_SECS: u64 = 2_400; // 40 minutes wall-clock cap
const LLM_STREAM_CHUNK_IDLE_TIMEOUT_SECS: u64 = 180; // 3 minutes max silence between SSE chunks

/// Call LLM using the Anthropic Messages API format with `stream: true` and an explicit `max_tokens` budget.
/// Uses per-chunk idle timeout (`LLM_STREAM_CHUNK_IDLE_TIMEOUT_SECS`) so 2-4 hour podcast generations
/// taking 10-25 minutes are never cut off as long as SSE chunks continue to arrive.
async fn call_llm_anthropic_with_tokens(
    config: &LlmConfig,
    system: &str,
    messages: &[serde_json::Value],
    max_tokens: u32,
) -> Result<String, String> {
    let api_key = match &config.api_key {
        Some(k) => k,
        None => return Err("AI not configured. Set LLM_API_KEY to enable.".into()),
    };

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(LLM_STREAM_TOTAL_TIMEOUT_SECS))
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let response = client
        .post(format!("{}/v1/messages", config.base_url))
        .header("x-api-key", api_key.as_str())
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": config.model,
            "max_tokens": max_tokens,
            "stream": true,
            "system": system,
            "messages": messages,
        }))
        .send()
        .await;

    match response {
        Ok(mut res) => {
            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                tracing::error!(%status, %body, "LLM API error");
                return Err(format!("LLM API error: {status}. {body}"));
            }
            let mut raw_bytes = Vec::new();
            let idle_timeout = std::time::Duration::from_secs(LLM_STREAM_CHUNK_IDLE_TIMEOUT_SECS);
            loop {
                match tokio::time::timeout(idle_timeout, res.chunk()).await {
                    Ok(Ok(Some(chunk))) => raw_bytes.extend_from_slice(&chunk),
                    Ok(Ok(None)) => break,
                    Ok(Err(e)) => return Err(format!("Failed to read LLM response stream: {e}")),
                    Err(_) => {
                        return Err(format!(
                            "LLM stream idle timeout (no data received for {LLM_STREAM_CHUNK_IDLE_TIMEOUT_SECS}s)"
                        ));
                    }
                }
            }
            let body = String::from_utf8_lossy(&raw_bytes);
            extract_llm_text_from_sse_or_json(&body)
        }
        Err(e) => Err(format!("Failed to call LLM: {e}")),
    }
}

/// Call LLM using default 16,384 token budget (for chat and chapter deep-read).
async fn call_llm_anthropic(
    config: &LlmConfig,
    system: &str,
    messages: &[serde_json::Value],
) -> Result<String, String> {
    call_llm_anthropic_with_tokens(config, system, messages, 16_384).await
}

// ---- Episode Handlers ----

#[derive(Deserialize)]
struct ListParams {
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_episodes(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);
    match state
        .store
        .list_episodes_for_user(limit, offset, Some(&viewer.user_id), viewer.is_admin())
        .await
    {
        Ok(episodes) => {
            let total = state
                .store
                .episode_count_for_user(Some(&viewer.user_id), viewer.is_admin())
                .await
                .unwrap_or(0);
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
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    match state.store.get_episode(&id).await {
        Ok(mut ep) => {
            if !viewer.can_view_episode(&ep) {
                return err_404();
            }
            ep.is_read = state
                .store
                .get_user_episode_read(&viewer.user_id, &id)
                .await
                .unwrap_or(false);
            (StatusCode::OK, Json(serde_json::to_value(ep).unwrap())).into_response()
        }
        Err(lumen_pod_core::error::PodError::NotFound(_)) => err_404(),
        Err(e) => err_500(e),
    }
}

async fn create_episode(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(mut episode): Json<lumen_pod_core::episode::Episode>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    if episode.visibility == "private" && !viewer.can_publish_private_episodes() {
        return err_403("需要管理员授权「发布私密视频」权限 (can_publish_private)");
    }
    if episode.id.is_empty() {
        episode.id = uuid::Uuid::new_v4().to_string();
    }
    if episode.owner_id.is_none() && !viewer.is_api_token {
        episode.owner_id = Some(viewer.user_id.clone());
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
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(updates): Json<serde_json::Value>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };
    if !viewer.can_view_episode(&ep) {
        return err_404();
    }

    // 1. Per-user read state (any approved viewer can mark their own read/unread state)
    if let Some(v) = updates.get("isRead").or_else(|| updates.get("is_read")) {
        let is_read = v.as_bool().unwrap_or(false);
        if let Err(e) = state
            .store
            .set_user_episode_read(&viewer.user_id, &id, is_read)
            .await
        {
            return err_500(e);
        }
    }

    // 2. Check if there are any episode-level fields being modified
    let has_episode_mutation = updates
        .as_object()
        .map(|obj| obj.keys().any(|k| k != "isRead" && k != "is_read"))
        .unwrap_or(false);

    if has_episode_mutation {
        if !viewer.can_edit_episodes() {
            return err_403("需要管理员授权「修改视频」权限 (can_edit)");
        }
        if let Some(vis) = updates.get("visibility").and_then(|v| v.as_str()) {
            if vis == "private" && !viewer.can_publish_private_episodes() {
                return err_403("需要管理员授权「发布私密视频」权限 (can_publish_private)");
            }
        }
        // Strip isRead/is_read from episode table update so global row isn't polluted
        let mut clean_updates = updates.clone();
        if let Some(obj) = clean_updates.as_object_mut() {
            obj.remove("isRead");
            obj.remove("is_read");
            // If setting private and episode has no owner_id yet, assign current user
            if obj.get("visibility").and_then(|v| v.as_str()) == Some("private")
                && ep.owner_id.is_none()
                && !viewer.is_api_token
            {
                obj.insert(
                    "ownerId".to_string(),
                    serde_json::Value::String(viewer.user_id.clone()),
                );
            }
        }
        ep = match state.store.update_episode(&id, &clean_updates).await {
            Ok(updated) => updated,
            Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
            Err(e) => return err_500(e),
        };
    }

    ep.is_read = state
        .store
        .get_user_episode_read(&viewer.user_id, &id)
        .await
        .unwrap_or(false);
    (StatusCode::OK, Json(serde_json::to_value(ep).unwrap())).into_response()
}

async fn delete_episode(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };
    if !viewer.can_view_episode(&ep) {
        return err_404();
    }
    if !viewer.can_delete_episodes() {
        return err_403("需要管理员授权「删除视频」权限 (can_delete)");
    }

    match state.store.delete_episode(&id).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response(),
        Ok(false) => err_404(),
        Err(e) => err_500(e),
    }
}

// ---- Audio Upload ----

async fn upload_audio(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !viewer.can_edit_episodes() {
        return err_403("需要管理员授权「修改视频」权限 (can_edit)");
    }
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
    if !viewer.can_view_episode(&ep) {
        return err_404();
    }

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
    visibility: Option<String>,
}

async fn submit_url(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SubmitUrlRequest>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let visibility = match req.visibility.as_deref() {
        Some("private") => {
            if !viewer.can_publish_private_episodes() {
                return err_403("需要管理员授权「发布私密视频」权限 (can_publish_private)");
            }
            "private".to_string()
        }
        _ => "public".to_string(),
    };

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
        owner_id: if viewer.is_api_token {
            None
        } else {
            Some(viewer.user_id.clone())
        },
        visibility: visibility.clone(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    match state.store.insert_episode(&episode).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": episode.id,
                "url": url,
                "visibility": visibility,
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
    headers: axum::http::HeaderMap,
    Query(params): Query<LookupParams>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let url = params.url.trim().to_string();
    if url.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "url parameter required"}))).into_response();
    }

    // Try exact URL match first
    match state.store.find_episode_by_url(&url).await {
        Ok(Some(mut ep)) if viewer.can_view_episode(&ep) => {
            ep.is_read = state
                .store
                .get_user_episode_read(&viewer.user_id, &ep.id)
                .await
                .unwrap_or(false);
            (StatusCode::OK, Json(serde_json::to_value(ep).unwrap())).into_response()
        }
        Ok(_) => {
            // For YouTube, also try alternate URL forms
            if let Some(video_id) = extract_youtube_video_id(&url) {
                let alt_urls = [
                    format!("https://www.youtube.com/watch?v={video_id}"),
                    format!("https://youtube.com/watch?v={video_id}"),
                    format!("https://youtu.be/{video_id}"),
                ];
                for alt in &alt_urls {
                    if alt == &url { continue; }
                    if let Ok(Some(mut ep)) = state.store.find_episode_by_url(alt).await {
                        if viewer.can_view_episode(&ep) {
                            ep.is_read = state
                                .store
                                .get_user_episode_read(&viewer.user_id, &ep.id)
                                .await
                                .unwrap_or(false);
                            return (StatusCode::OK, Json(serde_json::to_value(ep).unwrap())).into_response();
                        }
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
    headers: axum::http::HeaderMap,
    Query(params): Query<SearchParams>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = params.limit.unwrap_or(10).min(50);
    match state
        .store
        .search_episodes_for_user(&params.q, limit, Some(&viewer.user_id), viewer.is_admin())
        .await
    {
        Ok(ids) => {
            let mut episodes = Vec::new();
            for id in &ids {
                if let Ok(ep) = state.store.get_episode(id).await {
                    if viewer.can_view_episode(&ep) {
                        episodes.push(ep);
                    }
                }
            }
            let _ = state
                .store
                .hydrate_episodes_read_state(&mut episodes, &viewer.user_id)
                .await;
            (
                StatusCode::OK,
                Json(serde_json::json!({"results": episodes})),
            )
                .into_response()
        }
        Err(e) => err_500(e),
    }
}

// ---- Chat with AI (Per-User Scoped) ----

#[derive(Deserialize)]
struct ChatRequest {
    #[allow(dead_code)]
    role: String,
    content: String,
}

async fn list_chat(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Ok(ep) = state.store.get_episode(&id).await {
        if !viewer.can_view_episode(&ep) {
            return err_404();
        }
    }
    match state
        .store
        .list_user_chat_messages(&id, &viewer.user_id)
        .await
    {
        Ok(messages) => (StatusCode::OK, Json(serde_json::json!({"messages": messages}))).into_response(),
        Err(e) => err_500(e),
    }
}

async fn create_chat_with_ai(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => {
            if !viewer.can_view_episode(&ep) {
                return err_404();
            }
            ep
        }
        Err(_) => {
            return err_404();
        }
    };

    let user_msg = match state
        .store
        .add_user_chat_message(&id, &viewer.user_id, "user", &req.content)
        .await
    {
        Ok(msg) => msg,
        Err(e) => return err_500(e),
    };

    let history = state
        .store
        .list_user_chat_messages(&id, &viewer.user_id)
        .await
        .unwrap_or_default();

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

    let assistant_content = match call_llm_anthropic(&state.llm, &system, &messages).await {
        Ok(text) => text,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": err })),
            )
                .into_response();
        }
    };

    let assistant_msg = match state
        .store
        .add_user_chat_message(&id, &viewer.user_id, "assistant", &assistant_content)
        .await
    {
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

// ---- UTF-8 Safe Truncation & Prompt Helpers ----

/// Safely truncate a UTF-8 string to at most `max_chars` Unicode scalar values (`char`s).
/// Never panics on multi-byte characters (CJK, emoji, etc.).
fn truncate_utf8_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

fn lang_instruction_for(lang: &str) -> String {
    match lang {
        "zh" => "You MUST write the entire response in Simplified Chinese (简体中文). All headings, narrative, and analysis must be in Chinese (except original quotes and pinned foreign terms in parentheses).".into(),
        "en" => "Write the entire response in English.".into(),
        "ja" => "日本語で書いてください。見出し、分析、テキストのすべてを日本語で記述してください。".into(),
        "ko" => "전체 응답을 한국어로 작성하세요.".into(),
        "es" => "Escribe toda la respuesta en español.".into(),
        "fr" => "Rédigez l'intégralité de la réponse en français.".into(),
        "de" => "Schreiben Sie die gesamte Antwort auf Deutsch.".into(),
        other => format!("You MUST write the entire response in {other}."),
    }
}

// ---- Chapter Deep Read ----

#[derive(Deserialize)]
struct DeepReadParams {
    lang: Option<String>,
    regenerate: Option<bool>,
}

fn deep_read_system_prompt(lang: &str) -> String {
    let lang_instruction = lang_instruction_for(lang);
    format!(
        "You are a master long-form writer and content analyst inspired by the YouMind v54 'Read-and-Done' philosophy.\n\
         Your goal is to turn a podcast/video chapter into a self-contained, deeply insightful section that is so clear and complete \
         that the reader does NOT need to watch/listen to the original segment unless there is irreplaceable visual content.\n\n\
         Follow these strict writing constraints:\n\
         1. **Scene-First Opening**: NEVER start with 'In this chapter...' or 'The speaker discusses...'. Open directly with a concrete scene, tension, counter-intuitive data point, or vivid question from the segment.\n\
         2. **Reasoning over Assertions**: Do not just list conclusions. Reconstruct the speaker's chain of reasoning (Why & How) — how they move from observation/problem to conclusion.\n\
         3. **Preserve the Primary Case**: Keep the main example, experiment, or story in full narrative detail; never compress a concrete case study into a dry one-sentence bullet.\n\
         4. **Term Pinning & ASR Correction**: Cross-check proper nouns against the episode title/description to fix ASR homophone errors. When a key concept or technical term appears for the first time, include the original term in parentheses, e.g. `认知负荷（Cognitive Load）`.\n\
         5. **Tri-part Notable Quotes**: Include 1-2 of the most impactful verbatim quotes formatted as a three-part blockquote. Use the exact sentence-level timestamp placed immediately before that quote in the transcript:\n\
            > 「Translated quote in target language」\n\
            > *\"Original verbatim quote\"*\n\
            > —— Speaker Name `[MM:SS]` (or `[H:MM:SS]` when >= 1 hour)\n\
         6. **High-Bar Visual Referrals**: Only if the speaker is demonstrating something that words cannot replace (e.g. live UI/code demo, physical experiment, complex chart), insert `🎬 [MM:SS 建议回看原片画面：reason]` (or `🎬 [H:MM:SS 建议回看原片画面：reason]` when >= 1 hour). Otherwise, explain everything thoroughly in text.\n\
         7. **Unified Timestamp Format**:\n\
            - Under 1 hour (`< 60:00`): ALWAYS use `[MM:SS]` (e.g. `[05:14]`, `[11:33]`). NEVER add a leading zero hour such as `[0:11:33]` or `[00:11:33]`.\n\
            - 1 hour or later (`>= 1:00:00`): ALWAYS use `[H:MM:SS]` (e.g. `[1:02:15]`).\n\n\
         {lang_instruction}"
    )
}

async fn chapter_deep_read(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((id, idx)): Path<(String, usize)>,
    Query(params): Query<DeepReadParams>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };
    if !viewer.can_view_episode(&ep) {
        return err_404();
    }

    if idx >= ep.chapters.len() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "chapter index out of range"})),
        )
            .into_response();
    }

    let chapter = &ep.chapters[idx];
    let regenerate = params.regenerate.unwrap_or(false);

    // Return cached deep read if exists, is non-empty/valid, and not regenerating
    if !regenerate {
        if let Some(cached) = &chapter.deep_read {
            let trimmed = cached.trim();
            if !trimmed.is_empty() && trimmed != "No response from AI" {
                return (StatusCode::OK, Json(serde_json::json!({"content": cached, "cached": true}))).into_response();
            }
        }
    }

    let lang = params.lang.as_deref().unwrap_or("zh");
    let transcript_excerpt = build_chapter_transcript(&ep, chapter);
    let system = deep_read_system_prompt(lang);

    let prompt = format!(
        "Write a self-contained deep-read analysis of this chapter:\n\n\
         Episode Title: {}\nAuthor / Channel: {}\n\
         Chapter: {}\nTime: [{} — {}]\nOriginal Summary: {}\n\n\
         Transcript (with timestamps):\n{}",
        ep.source.title,
        ep.source.author,
        chapter.title,
        fmt_dur(chapter.start_time),
        fmt_dur(chapter.end_time),
        chapter.summary,
        truncate_utf8_chars(&transcript_excerpt, 24_000)
    );

    let messages = vec![serde_json::json!({"role": "user", "content": prompt})];
    let content = match call_llm_anthropic_with_tokens(&state.llm, &system, &messages, 16_384).await {
        Ok(c) => c,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": err })),
            )
                .into_response();
        }
    };

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

// ---- Episode Long-Form Article (YouMind v54 Read-and-Done Style) ----

#[derive(Deserialize, Default)]
#[serde(default)]
struct ArticleParams {
    lang: Option<String>,
    focus: Option<String>,
    regenerate: Option<bool>,
    #[serde(rename = "async")]
    async_mode: Option<bool>,
}

fn article_system_prompt(lang: &str, focus: Option<&str>) -> String {
    let lang_instruction = lang_instruction_for(lang);
    let focus_instruction = match focus.map(str::trim).filter(|s| !s.is_empty()) {
        Some(f) => format!(
            "\nUSER FOCUS DIRECTIVE:\nThe reader specifically wants to focus on: \"{f}\". \
             Give extra depth, examples, and analytical weight to this angle while preserving the overall structure.\n"
        ),
        None => String::new(),
    };

    format!(
        "You are an elite long-form writer and knowledge curator modelled after the YouMind v54 'YouTube Video to Article' skill.\n\
         Your mission is to transform a video/podcast transcript into a standalone, 'Read-and-Done' (读完就够) in-depth article.\n\
         After reading your article, a reader should fully grasp the speaker's framework, reasoning process, concrete cases, and memorable quotes \
         WITHOUT needing to watch the original video.\n\n\
         You MUST strictly follow all 8 golden writing constraints:\n\
         1. **Scene-First Hook (具体画面开场)**:\n\
            - NEVER start with 'This video talks about...', 'In this podcast...', or a dry abstract.\n\
            - Open immediately with a concrete scene, striking conflict, counter-intuitive statistic, or specific anecdote from the episode so the reader can judge within 200 words why this piece is worth reading.\n\
         2. **Reasoning over Assertions (讲清观点背后的思考过程)**:\n\
            - Do not reduce the content to bullet-point conclusions. Walk the reader through HOW and WHY the speaker arrives at each insight: what conventional wisdom they challenge, what intermediate steps they take, and what trade-offs they weigh.\n\
         3. **1:1 Framework + Primary Case Preservation (1:1 保留讲者框架与主案例细节)**:\n\
            - Mirror the speaker's logical progression using clear `##` section headings.\n\
            - Under every major section, you MUST preserve the primary example, experiment, story, or dataset in full narrative detail. Never compress a core story into a single summary sentence.\n\
         4. **Term Pinning & ASR Verification (专有名词核对与原词锚定)**:\n\
            - Use the episode title, author, and description to correct ASR homophone typos in person names, company names, books, and technical terms.\n\
            - When a key concept or technical term first appears, attach the original term in parentheses: `中文译名（Original Term）`.\n\
         5. **Tri-part Notable Quotes (三段式高规格金句引用)**:\n\
            - Weave 3–6 of the most powerful verbatim quotes into the article using this exact three-line blockquote format (use the exact sentence-level timestamp placed immediately before that quoted sentence in the transcript, NOT the paragraph-start timestamp):\n\
              > 「精炼传神的中文译文（或目标语言译文）」\n\
              > *\"Exact original quote from the transcript\"*\n\
              > —— Speaker Name `[MM:SS]` (or `[H:MM:SS]` when >= 1 hour)\n\
            - If the source language and target language are the same, you may omit the duplicate line and keep the quote + `—— Speaker Name [MM:SS]` (or `[H:MM:SS]`).\n\
         6. **High-Bar Visual Referrals (高门槛的「回看原片画面」指针)**:\n\
            - Only when words genuinely cannot replace the visual (e.g. live code/UI demo, physical experiment, complex chart walkthrough), insert `🎬 [MM:SS 建议回看原片画面：具体原因]` (or `🎬 [H:MM:SS 建议回看原片画面：具体原因]` when >= 1 hour).\n\
            - For all verbal arguments and stories, explain them completely in text.\n\
         7. **Unified Timestamp Format (统一时间戳格式)**:\n\
            - For any timestamp **under 1 hour (`< 60:00`)**, ALWAYS format as `[MM:SS]` with two-digit minutes (e.g. `[05:14]`, `[11:33]`). **NEVER** prefix with a zero hour like `[0:11:33]` or `[00:11:33]`, even if the episode is over an hour long.\n\
            - For any timestamp **at or over 1 hour (`>= 1:00:00`)**, ALWAYS format as `[H:MM:SS]` (e.g. `[1:02:15]`, `[2:10:05]`).\n\
         8. **Structured Post-Article Curations (结构化尾注：要点 + 书影音清单 + 延伸探索)**:\n\
            At the end of the article, always include these three curated sections:\n\
            - `## 💡 值得记住的核心认知` (Key Points Worth Remembering): 3–5 sharp cognitive takeaways.\n\
            - `## 📚 视频提及的书影音与工具` (Mentioned Books, Papers, Media & Tools): A structured list of books, papers, films, products, tools, or notable figures mentioned in the episode with their original names and context (if none were mentioned, state so briefly).\n\
            - `## 🧭 延伸探索建议` (Further Exploration): 2–3 genuinely relevant follow-up topics, search queries, or questions for active learning.\n\
         9. **Clean Title**:\n\
            - Start the response with a single `# Title` line that is compelling and faithful to the core thesis (avoid clickbait), followed by the article body.\n\
         {focus_instruction}\n\
         {lang_instruction}"
    )
}

/// Format a transcript paragraph with sentence-level timestamps (`[MM:SS]` or `[H:MM:SS]`)
/// so that quotes from the middle or end of a long paragraph carry the exact sentence timestamp
/// rather than the paragraph's opening timestamp.
fn format_paragraph_with_sentence_timestamps(
    para: &lumen_pod_core::episode::Paragraph,
) -> String {
    let mut line = String::new();
    let para_start = para
        .sentences
        .first()
        .and_then(|s| s.words.first().map(|w| w.start))
        .unwrap_or(0.0);
    let mut last_ts = fmt_dur(para_start);
    line.push_str(&format!("[{last_ts}] "));
    if let Some(speaker) = &para.speaker {
        line.push_str(&format!("{speaker}: "));
    }
    for (idx, sent) in para.sentences.iter().enumerate() {
        if idx > 0 {
            if let Some(sec) = sent.words.first().map(|w| w.start) {
                let ts = fmt_dur(sec);
                if ts != last_ts {
                    line.push_str(&format!("[{ts}] "));
                    last_ts = ts;
                }
            }
        }
        line.push_str(&sent.text);
        line.push(' ');
    }
    line.push('\n');
    line
}

fn build_episode_article_context(ep: &lumen_pod_core::episode::Episode) -> String {
    let mut ctx = String::new();
    ctx.push_str(&format!("Episode Title: {}\n", ep.source.title));
    ctx.push_str(&format!("Author / Channel: {}\n", ep.source.author));
    if let Some(pod) = &ep.source.podcast_name {
        ctx.push_str(&format!("Podcast / Show: {pod}\n"));
    }
    ctx.push_str(&format!("Duration: {}\n", fmt_dur(ep.source.duration)));
    if let Some(desc) = &ep.source.description {
        if !desc.trim().is_empty() {
            ctx.push_str(&format!(
                "Original Description / Show Notes:\n{}\n\n",
                truncate_utf8_chars(desc, 4_000)
            ));
        }
    }
    if let Some(s) = &ep.summary {
        ctx.push_str(&format!("Executive One-liner: {}\nAbstract: {}\n\n", s.one_liner, s.r#abstract));
    }
    if !ep.chapters.is_empty() {
        ctx.push_str("Chapters Outline:\n");
        for (i, ch) in ep.chapters.iter().enumerate() {
            ctx.push_str(&format!(
                "{}. [{} — {}] {}: {}\n",
                i + 1,
                fmt_dur(ch.start_time),
                fmt_dur(ch.end_time),
                ch.title,
                ch.summary
            ));
        }
        ctx.push('\n');
    }

    if let Some(t) = &ep.transcript {
        ctx.push_str("Full Timestamped Transcript (sentence-level timestamps):\n");
        let mut trans_buf = String::new();
        for para in &t.paragraphs {
            trans_buf.push_str(&format_paragraph_with_sentence_timestamps(para));
        }
        ctx.push_str(truncate_utf8_chars(&trans_buf, 240_000));
    }

    ctx
}

fn extract_markdown_title(markdown: &str, fallback_title: &str) -> (String, String) {
    let trimmed = markdown.trim();
    if let Some(rest) = trimmed.strip_prefix("# ") {
        if let Some((first_line, body)) = rest.split_once('\n') {
            let title = first_line.trim().to_string();
            if !title.is_empty() {
                return (title, body.trim_start().to_string());
            }
        }
    }
    (fallback_title.to_string(), trimmed.to_string())
}

async fn episode_article(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Query(query_params): Query<ArticleParams>,
    body_params: Option<Json<ArticleParams>>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };
    if !viewer.can_view_episode(&ep) {
        return err_404();
    }

    let body = body_params.map(|b| b.0).unwrap_or_default();
    let lang = body
        .lang
        .or(query_params.lang)
        .unwrap_or_else(|| "zh".to_string());
    let focus = body
        .focus
        .or(query_params.focus)
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty());
    let regenerate = body.regenerate.or(query_params.regenerate).unwrap_or(false);
    let async_mode = body.async_mode.or(query_params.async_mode).unwrap_or(false);

    if !regenerate && focus.is_none() {
        if let Some(existing) = &ep.article {
            let trimmed = existing.content.trim();
            if !trimmed.is_empty()
                && trimmed != "No response from AI"
                && existing.lang.as_deref().unwrap_or("zh") == lang
            {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "complete",
                        "article": existing,
                        "cached": true
                    })),
                )
                    .into_response();
            }
        }
    }

    // Check if an async generation task is already running for this episode (within ARTICLE_TASK_TIMEOUT_MS)
    {
        let tasks = state.article_tasks.lock().unwrap();
        if let Some(t) = tasks.get(&id) {
            if t.status == "running" && epoch_ms().saturating_sub(t.started_at_ms) < ARTICLE_TASK_TIMEOUT_MS {
                return (
                    StatusCode::ACCEPTED,
                    Json(serde_json::json!({
                        "status": "running",
                        "elapsedMs": epoch_ms().saturating_sub(t.started_at_ms)
                    })),
                )
                    .into_response();
            }
        }
    }

    let system = article_system_prompt(&lang, focus.as_deref());
    let context = build_episode_article_context(&ep);
    let user_prompt = format!(
        "Write a complete, standalone 'Read-and-Done' long-form article for the following episode.\n\
         Use the exact sentence-level timestamps from the transcript on quotes and visual referrals: `[MM:SS]` when under 1 hour (NEVER `[0:MM:SS]`) and `[H:MM:SS]` when 1 hour or later.\n\n\
         {context}"
    );

    if async_mode {
        let now_ms = epoch_ms();
        {
            let mut tasks = state.article_tasks.lock().unwrap();
            tasks.insert(
                id.clone(),
                auth::ArticleGenStatus {
                    status: "running".to_string(),
                    error: None,
                    started_at_ms: now_ms,
                },
            );
        }

        let state_clone = state.clone();
        let id_clone = id.clone();
        let fallback_title = ep.source.title.clone();
        tokio::spawn(async move {
            let messages = vec![serde_json::json!({"role": "user", "content": user_prompt})];
            match call_llm_anthropic_with_tokens(&state_clone.llm, &system, &messages, 64_000).await
            {
                Ok(raw_output) => {
                    let (title, content) = extract_markdown_title(&raw_output, &fallback_title);
                    let article = lumen_pod_core::episode::Article {
                        title,
                        content,
                        format: Some("markdown".to_string()),
                        lang: Some(lang),
                        focus,
                        generated_at: Some(chrono::Utc::now().to_rfc3339()),
                    };
                    let updates = serde_json::json!({
                        "article": serde_json::to_value(&article).unwrap_or_default()
                    });
                    if let Err(e) = state_clone.store.update_episode(&id_clone, &updates).await {
                        tracing::warn!(id = %id_clone, error = %e, "failed to persist generated article");
                    }
                    let mut tasks = state_clone.article_tasks.lock().unwrap();
                    tasks.remove(&id_clone);
                }
                Err(err) => {
                    tracing::error!(id = %id_clone, error = %err, "async article generation failed");
                    let mut tasks = state_clone.article_tasks.lock().unwrap();
                    tasks.insert(
                        id_clone,
                        auth::ArticleGenStatus {
                            status: "error".to_string(),
                            error: Some(err),
                            started_at_ms: now_ms,
                        },
                    );
                }
            }
        });

        return (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "running",
                "elapsedMs": 0
            })),
        )
            .into_response();
    }

    let messages = vec![serde_json::json!({"role": "user", "content": user_prompt})];
    // Use 64,000 max_tokens so reasoning models (e.g. MiniMax-M3.1) have ample budget for both thinking and 2-4h long-form article output
    let raw_output = match call_llm_anthropic_with_tokens(&state.llm, &system, &messages, 64_000).await {
        Ok(text) => text,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": err })),
            )
                .into_response();
        }
    };

    let (title, content) = extract_markdown_title(&raw_output, &ep.source.title);
    let article = lumen_pod_core::episode::Article {
        title,
        content,
        format: Some("markdown".to_string()),
        lang: Some(lang),
        focus,
        generated_at: Some(chrono::Utc::now().to_rfc3339()),
    };

    let updates = serde_json::json!({
        "article": serde_json::to_value(&article).unwrap_or_default()
    });
    if let Err(e) = state.store.update_episode(&id, &updates).await {
        tracing::warn!(%id, error = %e, "failed to persist generated article");
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "complete",
            "article": article,
            "cached": false
        })),
    )
        .into_response()
}

async fn episode_article_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };
    if !viewer.can_view_episode(&ep) {
        return err_404();
    }

    {
        let tasks = state.article_tasks.lock().unwrap();
        if let Some(t) = tasks.get(&id) {
            if t.status == "running" && epoch_ms().saturating_sub(t.started_at_ms) < ARTICLE_TASK_TIMEOUT_MS {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "running",
                        "elapsedMs": epoch_ms().saturating_sub(t.started_at_ms)
                    })),
                )
                    .into_response();
            }
            if t.status == "error" {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": t.error
                    })),
                )
                    .into_response();
            }
        }
    }

    if let Some(art) = &ep.article {
        let trimmed = art.content.trim();
        if !trimmed.is_empty() && trimmed != "No response from AI" {
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "complete",
                    "article": art
                })),
            )
                .into_response();
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "idle"
        })),
    )
        .into_response()
}

// ---- Batch Deep Read ----

#[derive(Deserialize)]
struct BatchDeepReadParams {
    lang: Option<String>,
}

async fn batch_deep_read_start(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<BatchDeepReadParams>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let lang = params.lang.unwrap_or_else(|| "zh".into());
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };
    if !viewer.can_view_episode(&ep) {
        return err_404();
    }

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
            "Write a self-contained deep-read analysis of this chapter:\n\n\
             Episode Title: {}\nAuthor / Channel: {}\n\
             Chapter: {}\nTime: [{} — {}]\nOriginal Summary: {}\n\n\
             Transcript (with timestamps):\n{}",
            ep.source.title,
            ep.source.author,
            chapter.title,
            fmt_dur(chapter.start_time),
            fmt_dur(chapter.end_time),
            chapter.summary,
            truncate_utf8_chars(&transcript_excerpt, 16_000)
        );

        let messages = vec![serde_json::json!({"role": "user", "content": prompt})];

        // Timeout: 3 minutes per chapter
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(180),
            call_llm_anthropic_with_tokens(&state.llm, &system, &messages, 16_384),
        ).await;

        let is_ok = match result {
            Ok(Ok(content)) => {
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
            Ok(Err(err_content)) => {
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
            text.push_str(&format_paragraph_with_sentence_timestamps(para));
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
    headers: axum::http::HeaderMap,
    Path((id, format)): Path<(String, String)>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ep = match state.store.get_episode(&id).await {
        Ok(ep) => ep,
        Err(lumen_pod_core::error::PodError::NotFound(_)) => return err_404(),
        Err(e) => return err_500(e),
    };
    if !viewer.can_view_episode(&ep) {
        return err_404();
    }

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
        for ch in &ep.chapters {
            writeln!(md, "- **[{}]** {} — {}", fmt_dur(ch.start_time), ch.title, ch.summary).ok();
            if let Some(dr) = &ch.deep_read {
                if !dr.trim().is_empty() {
                    writeln!(md, "\n{}\n", dr.trim()).ok();
                }
            }
        }
        writeln!(md).ok();
    }

    if let Some(art) = &ep.article {
        if !art.content.trim().is_empty() {
            writeln!(md, "## Article: {}\n\n{}\n", art.title, art.content.trim()).ok();
        }
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
        for ch in &ep.chapters {
            writeln!(md, "### {} ({})\n{}\n", ch.title, fmt_dur(ch.start_time), ch.summary).ok();
            if let Some(dr) = &ch.deep_read {
                if !dr.trim().is_empty() {
                    writeln!(md, "{}\n", dr.trim()).ok();
                }
            }
        }
    }
    if let Some(art) = &ep.article {
        if !art.content.trim().is_empty() {
            writeln!(md, "## In-Depth Article: {}\n\n{}\n", art.title, art.content.trim()).ok();
        }
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
        for ch in &ep.chapters {
            writeln!(md, "### {} `{}`\n{}\n", ch.title, fmt_dur(ch.start_time), ch.summary).ok();
            if let Some(dr) = &ch.deep_read {
                if !dr.trim().is_empty() {
                    writeln!(md, "{}\n", dr.trim()).ok();
                }
            }
        }
    }
    if let Some(art) = &ep.article {
        if !art.content.trim().is_empty() {
            writeln!(md, "## Article: {}\n\n{}\n", art.title, art.content.trim()).ok();
        }
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
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
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
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateCollectionRequest>,
) -> impl IntoResponse {
    let viewer = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !viewer.can_edit_episodes() {
        return err_403("需要管理员授权「修改视频/合集」权限 (can_edit)");
    }
    match state.store.create_collection(&req.name, req.description.as_deref()).await {
        Ok(col) => (StatusCode::CREATED, Json(serde_json::to_value(col).unwrap())).into_response(),
        Err(e) => err_500(e),
    }
}

// ---- Utils ----

async fn stats(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let (uid, is_admin) = match auth::resolve_viewer(&state, &headers).await {
        Ok(v) => (Some(v.user_id.clone()), v.is_admin()),
        Err(_) => (None, false),
    };
    let count = state
        .store
        .episode_count_for_user(uid.as_deref(), is_admin)
        .await
        .unwrap_or(0);
    Json(serde_json::json!({"episode_count": count}))
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

fn err_403(msg: &str) -> axum::response::Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"error": msg})),
    )
        .into_response()
}

fn err_404() -> axum::response::Response {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))).into_response()
}

fn err_500(e: impl std::fmt::Display) -> axum::response::Response {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_utf8_chars_handles_multibyte_and_emoji_safely() {
        let s = "认知负荷（Cognitive Load）🎬测试文本";
        // Truncating right in the middle of Chinese / emoji characters must never panic
        for max in 0..=s.chars().count() + 5 {
            let truncated = truncate_utf8_chars(s, max);
            assert!(truncated.chars().count() <= max);
            assert!(s.starts_with(truncated));
        }
        assert_eq!(truncate_utf8_chars(s, 4), "认知负荷");
    }

    #[test]
    fn test_extract_markdown_title() {
        let md = "# 为什么 90% 的播客摘要都漏掉了真正的推理链？\n\n正文第一段从具体画面开始。";
        let (title, body) = extract_markdown_title(md, "Fallback");
        assert_eq!(title, "为什么 90% 的播客摘要都漏掉了真正的推理链？");
        assert_eq!(body, "正文第一段从具体画面开始。");

        let no_heading = "直接开始正文内容";
        let (title2, body2) = extract_markdown_title(no_heading, "Fallback Title");
        assert_eq!(title2, "Fallback Title");
        assert_eq!(body2, "直接开始正文内容");
    }

    #[test]
    fn test_youmind_v54_prompts_include_8_constraints() {
        let article_prompt = article_system_prompt("zh", Some("Agent 架构设计"));
        assert!(article_prompt.contains("Scene-First Hook"));
        assert!(article_prompt.contains("Reasoning over Assertions"));
        assert!(article_prompt.contains("1:1 Framework + Primary Case Preservation"));
        assert!(article_prompt.contains("Term Pinning"));
        assert!(article_prompt.contains("Tri-part Notable Quotes"));
        assert!(article_prompt.contains("🎬"));
        assert!(article_prompt.contains("值得记住的核心认知"));
        assert!(article_prompt.contains("视频提及的书影音与工具"));
        assert!(article_prompt.contains("延伸探索建议"));
        assert!(article_prompt.contains("Agent 架构设计"));

        let chapter_prompt = deep_read_system_prompt("zh");
        assert!(chapter_prompt.contains("Scene-First Opening"));
        assert!(chapter_prompt.contains("Reasoning over Assertions"));
        assert!(chapter_prompt.contains("Tri-part Notable Quotes"));
    }

    #[test]
    fn test_viewer_rbac_and_private_visibility() {
        use lumen_pod_core::episode::{Episode, Processing, ProcessingStatus, Source, UserRole};

        let make_ep = |owner: Option<&str>, vis: &str| Episode {
            id: "ep-1".into(),
            source: Source {
                url: "https://youtube.com/watch?v=123".into(),
                platform: "youtube".into(),
                title: "Test".into(),
                author: "Author".into(),
                published_at: None,
                thumbnail_url: None,
                duration: 100.0,
                feed_url: None,
                description: None,
                podcast_name: None,
                episode_cover_url: None,
                language: None,
                audio_path: None,
            },
            processing: Processing {
                status: ProcessingStatus::Done,
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
            tags: vec![],
            collection_id: None,
            owner_id: owner.map(String::from),
            visibility: vis.into(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let admin = auth::ViewerContext {
            user_id: "u-admin".into(),
            email: "admin@test.com".into(),
            role: UserRole::Admin,
            approved: true,
            can_edit: true,
            can_delete: true,
            can_publish_private: true,
            is_api_token: false,
        };
        let user_a = auth::ViewerContext {
            user_id: "u-a".into(),
            email: "a@test.com".into(),
            role: UserRole::Member,
            approved: true,
            can_edit: false,
            can_delete: false,
            can_publish_private: true,
            is_api_token: false,
        };
        let user_b = auth::ViewerContext {
            user_id: "u-b".into(),
            email: "b@test.com".into(),
            role: UserRole::Member,
            approved: true,
            can_edit: true,
            can_delete: false,
            can_publish_private: false,
            is_api_token: false,
        };

        let priv_ep = make_ep(Some("u-a"), "private");
        let pub_ep = make_ep(Some("u-a"), "public");

        // Private episode: visible ONLY to owner (user_a) and admin, NOT user_b
        assert!(user_a.can_view_episode(&priv_ep));
        assert!(admin.can_view_episode(&priv_ep));
        assert!(!user_b.can_view_episode(&priv_ep));

        // Public episode: visible to all approved users
        assert!(user_a.can_view_episode(&pub_ep));
        assert!(user_b.can_view_episode(&pub_ep));
        assert!(admin.can_view_episode(&pub_ep));

        // RBAC permissions
        assert!(admin.can_edit_episodes() && admin.can_delete_episodes() && admin.can_publish_private_episodes());
        assert!(!user_a.can_edit_episodes() && !user_a.can_delete_episodes() && user_a.can_publish_private_episodes());
        assert!(user_b.can_edit_episodes() && !user_b.can_delete_episodes() && !user_b.can_publish_private_episodes());
    }

    #[test]
    fn test_extract_llm_text_handles_thinking_only_exhaustion_and_valid_response() {
        // Case 1: Reasoning model exhausted max_tokens during thinking (only thinking block, no text block)
        let exhausted_json = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "Long internal chain of thought..."}
            ],
            "stop_reason": "max_tokens",
            "usage": {
                "input_tokens": 23622,
                "output_tokens": 8192,
                "output_tokens_details": {
                    "thinking_tokens": 8192
                }
            }
        });
        let err = extract_llm_text_from_response(&exhausted_json).unwrap_err();
        assert!(err.contains("stop_reason=max_tokens"));
        assert!(err.contains("thinking_tokens=8192"));

        // Case 2: Reasoning model finished thinking and emitted text block
        let ok_json = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "Internal reasoning"},
                {"type": "text", "text": "# 经验时代的黎明\n\n[1:02:15] 正文内容"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 23622,
                "output_tokens": 6500,
                "output_tokens_details": {
                    "thinking_tokens": 2100
                }
            }
        });
        let text = extract_llm_text_from_response(&ok_json).unwrap();
        assert_eq!(text, "# 经验时代的黎明\n\n[1:02:15] 正文内容");

        // Case 3: SSE stream with thinking deltas followed by text deltas
        let sse_ok = r##"
event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":23622}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Thinking deeply..."}}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"# 经验时代的黎明\n\n"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"正文第一段 [1:02:15]"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":23622,"output_tokens":4200,"output_tokens_details":{"thinking_tokens":1800}}}
"##;
        let sse_text = extract_llm_text_from_sse_or_json(sse_ok).unwrap();
        assert_eq!(sse_text, "# 经验时代的黎明\n\n正文第一段 [1:02:15]");
    }

    #[test]
    fn test_prompts_support_hour_plus_timestamps() {
        let article_prompt = article_system_prompt("zh", None);
        assert!(article_prompt.contains("[H:MM:SS]"));
        assert!(article_prompt.contains("[0:11:33]"));
        let chapter_prompt = deep_read_system_prompt("zh");
        assert!(chapter_prompt.contains("[H:MM:SS]"));
        assert!(chapter_prompt.contains("[0:11:33]"));
    }

    #[test]
    fn test_fmt_dur_and_sentence_level_timestamps() {
        use lumen_pod_core::episode::{Paragraph, Sentence, Word};

        assert_eq!(fmt_dur(0.0), "00:00");
        assert_eq!(fmt_dur(314.0), "05:14");
        assert_eq!(fmt_dur(693.0), "11:33");
        assert_eq!(fmt_dur(3987.0), "1:06:27");

        let para = Paragraph {
            id: 1,
            speaker: Some("Mark Zuckerberg".into()),
            sentences: vec![
                Sentence {
                    id: "s1".into(),
                    text: "When you're doing MMA, you get 60 seconds between rounds.".into(),
                    words: vec![Word {
                        id: "w1".into(),
                        text: "When".into(),
                        start: 693.2,
                        end: 693.5,
                    }],
                },
                Sentence {
                    id: "s2".into(),
                    text: "You're completely exhausted.".into(),
                    words: vec![Word {
                        id: "w2".into(),
                        text: "You're".into(),
                        start: 711.8,
                        end: 712.1,
                    }],
                },
                Sentence {
                    id: "s3".into(),
                    text: "And at the end of the hour we scale up.".into(),
                    words: vec![Word {
                        id: "w3".into(),
                        text: "And".into(),
                        start: 3987.4,
                        end: 3987.9,
                    }],
                },
            ],
        };

        let formatted = format_paragraph_with_sentence_timestamps(&para);
        assert_eq!(
            formatted,
            "[11:33] Mark Zuckerberg: When you're doing MMA, you get 60 seconds between rounds. [11:51] You're completely exhausted. [1:06:27] And at the end of the hour we scale up. \n"
        );
    }
}
