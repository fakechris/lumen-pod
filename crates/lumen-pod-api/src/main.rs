use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use lumen_pod_core::store::Store;

type AppState = Arc<Store>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lumen_pod_api=info,tower_http=info".into()),
        )
        .init();

    let db_path = std::env::var("LUMEN_POD_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_or_default().join("lumen-pod.db"));

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let store = Store::open(&db_path).expect("failed to open database");
    tracing::info!(path = %db_path.display(), "database opened");

    let state: AppState = Arc::new(store);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/episodes", get(list_episodes))
        .route("/api/episodes/{id}", get(get_episode).delete(delete_episode))
        .route("/api/search", get(search_episodes))
        .route("/api/stats", get(stats))
        .route("/healthz", get(healthz))
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

fn dirs_or_default() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join("Library/Application Support/LumenPod")
    }
    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::from(".")
    }
}

#[derive(Deserialize)]
struct ListParams {
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_episodes(
    State(store): State<AppState>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);
    match store.list_episodes(limit, offset) {
        Ok(episodes) => {
            let total = store.episode_count().unwrap_or(0);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "total": total,
                    "episodes": episodes,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_episode(
    State(store): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store.get_episode(&id) {
        Ok(ep) => (StatusCode::OK, Json(serde_json::to_value(ep).unwrap())).into_response(),
        Err(lumen_pod_core::error::PodError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_episode(
    State(store): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store.delete_episode(&id) {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct SearchParams {
    q: String,
    limit: Option<usize>,
}

async fn search_episodes(
    State(store): State<AppState>,
    Query(params): Query<SearchParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(10).min(50);
    match store.search_episodes(&params.q, limit) {
        Ok(ids) => {
            let episodes: Vec<_> = ids
                .iter()
                .filter_map(|id| store.get_episode(id).ok())
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({"results": episodes})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn stats(State(store): State<AppState>) -> impl IntoResponse {
    let count = store.episode_count().unwrap_or(0);
    Json(serde_json::json!({"episode_count": count}))
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}
