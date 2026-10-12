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

#[derive(Parser)]
#[command(name = "lumen-pod", version, about = "Agent-native podcast knowledge extraction")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the REST API server (default)
    Serve,
    /// Start the MCP stdio server for agent communication
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

    // Seed built-in admin(s)
    for email in &auth_config.admin_emails {
        if let Err(e) = store.seed_admin(email).await {
            tracing::warn!(%email, error = %e, "failed to seed admin");
        } else {
            tracing::info!(%email, "admin user seeded");
        }
    }

    let state = AppState {
        store: Arc::new(store),
        auth: auth_config,
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

    let app = Router::new()
        // Public
        .route("/healthz", get(healthz))
        .route("/auth/google", get(auth::google_login))
        .route("/auth/google/callback", get(auth::google_callback))
        // Authenticated
        .route("/api/me", get(auth::me))
        .route("/api/episodes", get(list_episodes).post(create_episode))
        .route(
            "/api/episodes/{id}",
            get(get_episode).patch(update_episode).delete(delete_episode),
        )
        .route(
            "/api/episodes/{id}/chat",
            get(list_chat).post(create_chat),
        )
        .route("/api/episodes/{id}/export/{format}", get(export_episode))
        .route("/api/search", get(search_episodes))
        .route("/api/stats", get(stats))
        .route(
            "/api/collections",
            get(list_collections).post(create_collection),
        )
        // Admin
        .route(
            "/api/invites",
            get(auth::list_invites).post(auth::create_invite),
        )
        .route("/api/admin/users", get(auth::list_users))
        .route(
            "/api/admin/users/{id}",
            axum::routing::patch(auth::update_user).delete(auth::delete_user),
        )
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

// ---- Chat ----

#[derive(Deserialize)]
struct ChatRequest {
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

async fn create_chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    match state.store.add_chat_message(&id, &req.role, &req.content).await {
        Ok(msg) => (StatusCode::CREATED, Json(serde_json::to_value(msg).unwrap())).into_response(),
        Err(e) => err_500(e),
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
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
                    (
                        header::CONTENT_DISPOSITION,
                        &format!("attachment; filename=\"{}.md\"", sanitize_filename(&ep.source.title)),
                    ),
                ],
                md,
            )
                .into_response()
        }
        "srt" => {
            let srt = build_srt(&ep);
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/x-subrip; charset=utf-8"),
                    (
                        header::CONTENT_DISPOSITION,
                        &format!("attachment; filename=\"{}.srt\"", sanitize_filename(&ep.source.title)),
                    ),
                ],
                srt,
            )
                .into_response()
        }
        "json" => {
            let json = serde_json::to_string_pretty(&ep).unwrap_or_default();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/json; charset=utf-8"),
                    (
                        header::CONTENT_DISPOSITION,
                        &format!("attachment; filename=\"{}.json\"", sanitize_filename(&ep.source.title)),
                    ),
                ],
                json,
            )
                .into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "unsupported format. use: markdown, srt, json"})),
        )
            .into_response(),
    }
}

fn build_markdown(ep: &lumen_pod_core::episode::Episode) -> String {
    use std::fmt::Write;
    let mut md = String::new();
    writeln!(md, "# {}\n", ep.source.title).ok();
    writeln!(md, "**Author:** {}  ", ep.source.author).ok();
    writeln!(md, "**Duration:** {}  ", crate::fmt_dur(ep.source.duration)).ok();
    writeln!(md, "**Source:** [{}]({})\n", ep.source.platform, ep.source.url).ok();

    if let Some(s) = &ep.summary {
        writeln!(md, "## Summary\n").ok();
        writeln!(md, "> {}\n", s.one_liner).ok();
        writeln!(md, "{}\n", s.r#abstract).ok();
        if !s.key_takeaways.is_empty() {
            writeln!(md, "### Key Takeaways\n").ok();
            for t in &s.key_takeaways {
                writeln!(md, "- {t}").ok();
            }
            writeln!(md).ok();
        }
        if !s.glossary.is_empty() {
            writeln!(md, "### Glossary\n").ok();
            for g in &s.glossary {
                writeln!(md, "- **{}**: {}", g.term, g.definition).ok();
            }
            writeln!(md).ok();
        }
    }

    if !ep.chapters.is_empty() {
        writeln!(md, "## Chapters\n").ok();
        for ch in &ep.chapters {
            writeln!(
                md,
                "- **[{}]** {} — {}",
                crate::fmt_dur(ch.start_time),
                ch.title,
                ch.summary
            )
            .ok();
        }
        writeln!(md).ok();
    }

    if !ep.keywords.is_empty() {
        writeln!(md, "## Keywords\n").ok();
        for kw in &ep.keywords {
            if let Some(desc) = &kw.desc {
                writeln!(md, "- **{}**: {desc}", kw.key).ok();
            } else {
                writeln!(md, "- **{}**", kw.key).ok();
            }
        }
        writeln!(md).ok();
    }

    if !ep.qas.is_empty() {
        writeln!(md, "## Q&A\n").ok();
        for qa in &ep.qas {
            writeln!(md, "**Q: {}**\n", qa.question).ok();
            writeln!(md, "A: {}\n", qa.answer).ok();
        }
    }

    if let Some(t) = &ep.transcript {
        writeln!(md, "## Transcript\n").ok();
        for para in &t.paragraphs {
            if let Some(speaker) = &para.speaker {
                write!(md, "**{speaker}:** ").ok();
            }
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
                if sent.words.is_empty() {
                    continue;
                }
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

fn srt_time(seconds: f64) -> String {
    let h = (seconds / 3600.0) as u32;
    let m = ((seconds % 3600.0) / 60.0) as u32;
    let s = (seconds % 60.0) as u32;
    let ms = ((seconds % 1.0) * 1000.0) as u32;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect::<String>()
        .trim()
        .to_string()
}

fn fmt_dur(seconds: f64) -> String {
    let h = (seconds / 3600.0) as u32;
    let m = ((seconds % 3600.0) / 60.0) as u32;
    let s = (seconds % 60.0) as u32;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
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
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": e.to_string()})),
    )
        .into_response()
}
