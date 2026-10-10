mod auth;

use std::net::SocketAddr;
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

use auth::AppState;
use lumen_pod_core::store::Store;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lumen_pod_api=info,tower_http=info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://lumenpod:lumenpod@127.0.0.1:5488/lumenpod".into());

    let store = Store::new(&database_url)
        .await
        .expect("failed to connect to database");
    tracing::info!(url = %database_url, "database connected");

    let state = AppState {
        store: Arc::new(store),
        auth: auth::AuthConfig::from_env(),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

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
        .route("/api/search", get(search_episodes))
        .route("/api/stats", get(stats))
        // Admin
        .route(
            "/api/invites",
            get(auth::list_invites).post(auth::create_invite),
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
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_episode(&id).await {
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn update_episode(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(updates): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state.store.update_episode(&id, &updates).await {
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
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.delete_episode(&id).await {
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn stats(State(state): State<AppState>) -> impl IntoResponse {
    let count = state.store.episode_count().await.unwrap_or(0);
    Json(serde_json::json!({"episode_count": count}))
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}
