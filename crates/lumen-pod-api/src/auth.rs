//! Google OAuth 2.0 + JWT session management.
//!
//! Auth flow: anyone can log in via Google → user record created →
//! built-in admins auto-approved → invited emails auto-approved →
//! others are "pending" and get 403 on content endpoints.

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use lumen_pod_core::episode::UserRole;
use lumen_pod_core::store::Store;

// ---- Configuration ----

#[derive(Clone)]
pub struct AuthConfig {
    pub google_client_id: String,
    pub google_client_secret: String,
    pub google_redirect_uri: String,
    pub jwt_secret: String,
    pub admin_emails: Vec<String>,
    pub api_tokens: Vec<String>,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let admin_emails: Vec<String> = std::env::var("ADMIN_EMAILS")
            .unwrap_or_else(|_| "fakechris@gmail.com".into())
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        let api_tokens: Vec<String> = std::env::var("LUMEN_POD_API_TOKEN")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let jwt_secret = std::env::var("JWT_SECRET")
            .unwrap_or_else(|_| "dev-secret-change-in-production".into());

        Self {
            google_client_id: std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default(),
            google_client_secret: std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default(),
            google_redirect_uri: std::env::var("GOOGLE_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:3210/auth/google/callback".into()),
            jwt_secret,
            admin_emails,
            api_tokens,
        }
    }

    pub fn validate(&self) {
        const DEFAULTS: &[&str] = &[
            "dev-secret-change-in-production",
            "change-me-in-production",
        ];
        if DEFAULTS.contains(&self.jwt_secret.as_str()) || self.jwt_secret.len() < 32 {
            panic!(
                "FATAL: JWT_SECRET is insecure (default or < 32 chars). \
                 Set a strong random value: openssl rand -hex 32"
            );
        }
    }
}

// ---- JWT Claims ----

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub role: String,
    pub approved: bool,
    pub exp: usize,
    pub iat: usize,
}

impl Claims {
    pub fn new(user_id: &str, email: &str, role: &str, approved: bool) -> Self {
        let now = Utc::now();
        Self {
            sub: user_id.into(),
            email: email.into(),
            role: role.into(),
            approved,
            iat: now.timestamp() as usize,
            exp: (now + Duration::days(7)).timestamp() as usize,
        }
    }
}

pub fn create_jwt(claims: &Claims, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

// ---- Auth middleware helpers ----

/// True when the request carries a valid `Authorization: Bearer <api-token>`
/// header matching one of the configured `LUMEN_POD_API_TOKEN` values.
pub fn matches_api_token(auth: &AuthConfig, headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .is_some_and(|token| {
            auth.api_tokens
                .iter()
                .any(|t| constant_time_eq(t.as_bytes(), token.as_bytes()))
        })
}

pub fn sha256_hex(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Resolve an admin-capable viewer from a Bearer API token: issued tokens
/// (hashed in the `api_tokens` table) first, then the `LUMEN_POD_API_TOKEN`
/// env values as operator bootstrap tokens. Usage is NOT counted here —
/// callers that own the request touch `viewer.api_token_id` exactly once
/// (handlers may resolve the viewer repeatedly).
pub async fn api_token_viewer(state: &AppState, headers: &HeaderMap) -> Option<ViewerContext> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))?;

    if let Ok(Some(row)) = state.store.get_api_token_by_hash(&sha256_hex(bearer)).await {
        let mut viewer = ViewerContext::api_token();
        viewer.api_token_id = Some(row.id);
        return Some(viewer);
    }

    matches_api_token(&state.auth, headers).then_some(ViewerContext::api_token())
}

/// Count one authenticated request for an issued token (fire-and-forget).
pub fn touch_api_token_usage(state: &AppState, viewer: &ViewerContext) {
    if let Some(id) = &viewer.api_token_id {
        let store = state.store.clone();
        let id = id.clone();
        tokio::spawn(async move {
            let _ = store.touch_api_token(&id).await;
        });
    }
}

pub fn extract_claims(headers: &HeaderMap, jwt_secret: &str) -> Option<Claims> {
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return verify_jwt(token, jwt_secret).ok();
            }
        }
    }

    if let Some(cookie) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie.to_str() {
            for part in cookie_str.split(';') {
                let part = part.trim();
                if let Some(token) = part.strip_prefix("lumen_pod_session=") {
                    return verify_jwt(token, jwt_secret).ok();
                }
            }
        }
    }

    None
}

// ---- Viewer Context (RBAC + Per-User Isolation) ----

#[derive(Debug, Clone)]
pub struct ViewerContext {
    pub user_id: String,
    pub email: String,
    pub role: UserRole,
    pub approved: bool,
    pub can_edit: bool,
    pub can_delete: bool,
    pub can_publish_private: bool,
    pub is_api_token: bool,
    /// `api_tokens.id` when authenticated via an issued (DB) token; None for
    /// env tokens. Callers that own the request touch usage exactly once.
    pub api_token_id: Option<String>,
}

impl ViewerContext {
    pub fn api_token() -> Self {
        Self {
            user_id: "api-token".to_string(),
            email: "agent@lumen-pod.local".to_string(),
            role: UserRole::Admin,
            approved: true,
            can_edit: true,
            can_delete: true,
            can_publish_private: true,
            is_api_token: true,
            api_token_id: None,
        }
    }

    pub fn is_admin(&self) -> bool {
        self.is_api_token || self.role == UserRole::Admin
    }

    pub fn can_edit_episodes(&self) -> bool {
        self.is_admin() || self.can_edit
    }

    pub fn can_delete_episodes(&self) -> bool {
        self.is_admin() || self.can_delete
    }

    pub fn can_publish_private_episodes(&self) -> bool {
        self.is_admin() || self.can_publish_private
    }

    pub fn can_view_episode(&self, ep: &lumen_pod_core::episode::Episode) -> bool {
        if ep.visibility != "private" {
            return true;
        }
        self.is_admin() || ep.owner_id.as_deref() == Some(self.user_id.as_str())
    }
}

/// Resolve the current viewer from Bearer API token or JWT session + live DB permissions.
pub async fn resolve_viewer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<ViewerContext, axum::response::Response> {
    if let Some(viewer) = api_token_viewer(state, headers).await {
        return Ok(viewer);
    }

    let Some(claims) = extract_claims(headers, &state.auth.jwt_secret) else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "not authenticated"})),
        )
            .into_response());
    };

    // Fetch live user record from DB so permission/role/approval changes take effect immediately
    let db_user = match state.store.get_user_by_id(&claims.sub).await {
        Ok(Some(u)) => Some(u),
        _ => state.store.get_user_by_email(&claims.email).await.ok().flatten(),
    };

    let viewer = if let Some(u) = db_user {
        let is_admin = u.role == UserRole::Admin;
        ViewerContext {
            user_id: u.id,
            email: u.email,
            role: u.role,
            approved: u.approved,
            can_edit: is_admin || u.can_edit,
            can_delete: is_admin || u.can_delete,
            can_publish_private: is_admin || u.can_publish_private,
            is_api_token: false,
            api_token_id: None,
        }
    } else {
        let is_admin = claims.role == "admin";
        ViewerContext {
            user_id: claims.sub,
            email: claims.email,
            role: if is_admin { UserRole::Admin } else { UserRole::Member },
            approved: claims.approved,
            can_edit: is_admin,
            can_delete: is_admin,
            can_publish_private: is_admin,
            is_api_token: false,
            api_token_id: None,
        }
    };

    if !viewer.approved {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "pending_approval",
                "message": "Your account is awaiting admin approval.",
                "email": viewer.email,
            })),
        )
            .into_response());
    }

    Ok(viewer)
}

/// Require authenticated + approved + admin role (resolves live DB role).
pub async fn require_admin_viewer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<ViewerContext, axum::response::Response> {
    let viewer = resolve_viewer(state, headers).await?;
    if !viewer.is_admin() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "admin only"})),
        )
            .into_response());
    }
    Ok(viewer)
}

// ---- Auth middleware (axum layer) ----

/// Middleware: require either a valid API token (Bearer) or an approved session (resolved from DB).
/// Attach to all /api/* routes except /api/me.
pub async fn require_auth(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, axum::response::Response> {
    let headers = req.headers();
    match resolve_viewer(&state, headers).await {
        Ok(viewer) => {
            touch_api_token_usage(&state, &viewer);
            Ok(next.run(req).await)
        }
        Err(e) => Err(e),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ---- Google OAuth Handlers ----

#[derive(Clone)]
pub struct ArticleGenStatus {
    pub status: String, // "running" | "error"
    pub error: Option<String>,
    pub started_at_ms: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub auth: AuthConfig,
    pub llm: crate::LlmConfig,
    pub audio_dir: String,
    pub static_dir: String,
    pub batch_tasks: Arc<std::sync::Mutex<std::collections::HashMap<String, crate::BatchDeepReadTask>>>,
    pub article_tasks: Arc<std::sync::Mutex<std::collections::HashMap<String, ArticleGenStatus>>>,
    pub chapter_tasks: Arc<std::sync::Mutex<std::collections::HashMap<String, ArticleGenStatus>>>,
}

impl AppState {
    /// LLM config with admin-editable DB settings overriding the env defaults
    /// (`llm_model`, `llm_base_url`; empty or missing rows fall back to env).
    pub async fn effective_llm(&self) -> crate::LlmConfig {
        let mut cfg = self.llm.clone();
        if let Ok(Some(v)) = self.store.get_setting("llm_model").await {
            let v = v.trim().to_string();
            if !v.is_empty() {
                cfg.model = v;
            }
        }
        if let Ok(Some(v)) = self.store.get_setting("llm_base_url").await {
            let v = v.trim().to_string();
            if !v.is_empty() {
                cfg.base_url = v;
            }
        }
        cfg
    }
}

pub async fn google_login(State(state): State<AppState>) -> impl IntoResponse {
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?\
         client_id={}&\
         redirect_uri={}&\
         response_type=code&\
         scope=email%20profile&\
         access_type=offline&\
         prompt=consent",
        state.auth.google_client_id,
        urlencoded(&state.auth.google_redirect_uri),
    );
    Redirect::temporary(&url)
}

#[derive(Deserialize)]
pub struct OAuthCallback {
    pub code: String,
}

#[derive(Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct GoogleUserInfo {
    #[serde(default)]
    sub: String,
    email: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    picture: Option<String>,
}

pub async fn google_callback(
    State(state): State<AppState>,
    Query(params): Query<OAuthCallback>,
) -> impl IntoResponse {
    let client = reqwest::Client::new();

    let token_resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", params.code.as_str()),
            ("client_id", &state.auth.google_client_id),
            ("client_secret", &state.auth.google_client_secret),
            ("redirect_uri", &state.auth.google_redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await;

    let token_resp = match token_resp {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("token exchange failed: {e}")})),
            )
                .into_response();
        }
    };

    let token: GoogleTokenResponse = match token_resp.json().await {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("bad token response: {e}")})),
            )
                .into_response();
        }
    };

    let user_resp = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(&token.access_token)
        .send()
        .await;

    let user_info: GoogleUserInfo = match user_resp {
        Ok(r) => match r.json().await {
            Ok(u) => u,
            Err(e) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({"error": format!("bad userinfo: {e}")})),
                )
                    .into_response();
            }
        },
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("userinfo failed: {e}")})),
            )
                .into_response();
        }
    };

    // Let everyone in — admin/invited auto-approved, others pending
    let user = match state
        .store
        .upsert_user_google(
            &user_info.email,
            user_info.name.as_deref(),
            user_info.picture.as_deref(),
            &user_info.sub,
            &state.auth.admin_emails,
        )
        .await
    {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("user upsert failed: {e}")})),
            )
                .into_response();
        }
    };

    let role_str = match user.role {
        UserRole::Admin => "admin",
        UserRole::Member => "member",
    };

    let claims = Claims::new(&user.id, &user.email, role_str, user.approved);
    let jwt = match create_jwt(&claims, &state.auth.jwt_secret) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("jwt error: {e}")})),
            )
                .into_response();
        }
    };

    let secure = state.auth.google_redirect_uri.starts_with("https://");
    let cookie = format!(
        "lumen_pod_session={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800{}",
        jwt,
        if secure { "; Secure" } else { "" },
    );

    // Derive frontend base URL from the redirect URI (strip /auth/google/callback)
    let frontend_url = state
        .auth
        .google_redirect_uri
        .trim_end_matches("/auth/google/callback")
        .to_string();
    let redirect_to = if frontend_url.is_empty() {
        "/".to_string()
    } else {
        frontend_url
    };

    (
        StatusCode::FOUND,
        [
            (header::SET_COOKIE, cookie),
            (header::LOCATION, redirect_to),
        ],
    )
        .into_response()
}

/// GET /api/me — always works for authenticated users (even pending).
/// Also accepts a Bearer API token so agents can validate their token here
/// (returning 401 for a valid token reads as "rejected" to API clients).
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(viewer) = api_token_viewer(&state, &headers).await {
        touch_api_token_usage(&state, &viewer);
        let viewer = ViewerContext::api_token();
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": viewer.user_id,
                "email": viewer.email,
                "role": "admin",
                "approved": true,
                "can_edit": true,
                "can_delete": true,
                "can_publish_private": true,
            })),
        )
            .into_response();
    }

    match extract_claims(&headers, &state.auth.jwt_secret) {
        Some(claims) => {
            let user = state.store.get_user_by_email(&claims.email).await;
            match user {
                Ok(Some(u)) => {
                    let is_admin = u.role == UserRole::Admin;
                    let role_str = match u.role {
                        UserRole::Admin => "admin",
                        UserRole::Member => "member",
                    };
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": u.id,
                            "email": u.email,
                            "name": u.name,
                            "avatar_url": u.avatar_url,
                            "role": role_str,
                            "approved": u.approved,
                            "can_edit": is_admin || u.can_edit,
                            "can_delete": is_admin || u.can_delete,
                            "can_publish_private": is_admin || u.can_publish_private,
                        })),
                    )
                        .into_response()
                }
                _ => {
                    let is_admin = claims.role == "admin";
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": claims.sub,
                            "email": claims.email,
                            "role": claims.role,
                            "approved": claims.approved,
                            "can_edit": is_admin,
                            "can_delete": is_admin,
                            "can_publish_private": is_admin,
                        })),
                    )
                        .into_response()
                }
            }
        }
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "not authenticated"})),
        )
            .into_response(),
    }
}

// ---- Admin: User management ----

pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin_viewer(&state, &headers).await {
        return e;
    }

    match state.store.list_users().await {
        Ok(users) => {
            let data: Vec<_> = users
                .iter()
                .map(|u| {
                    let is_admin = u.role == UserRole::Admin;
                    serde_json::json!({
                        "id": u.id,
                        "email": u.email,
                        "name": u.name,
                        "avatar_url": u.avatar_url,
                        "role": match u.role { UserRole::Admin => "admin", UserRole::Member => "member" },
                        "approved": u.approved,
                        "can_edit": is_admin || u.can_edit,
                        "can_delete": is_admin || u.can_delete,
                        "can_publish_private": is_admin || u.can_publish_private,
                        "created_at": u.created_at,
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"users": data}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    pub approved: Option<bool>,
    pub role: Option<String>,
    #[serde(alias = "canEdit")]
    pub can_edit: Option<bool>,
    #[serde(alias = "canDelete")]
    pub can_delete: Option<bool>,
    #[serde(alias = "canPublishPrivate")]
    pub can_publish_private: Option<bool>,
}

pub async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_admin_viewer(&state, &headers).await {
        return e;
    }

    match state
        .store
        .update_user_permissions(
            &user_id,
            req.approved,
            req.role.as_deref(),
            req.can_edit,
            req.can_delete,
            req.can_publish_private,
        )
        .await
    {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"updated": true}))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "user not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_admin_viewer(&state, &headers).await {
        return e;
    }

    match state.store.reject_user(&user_id).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response(),
        Ok(false) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "cannot delete admin user or user not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ---- Invite management (admin only) ----

#[derive(Deserialize)]
pub struct CreateInviteRequest {
    pub email: String,
}

pub async fn create_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateInviteRequest>,
) -> impl IntoResponse {
    let viewer = match require_admin_viewer(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let invite = lumen_pod_core::episode::Invite {
        id: uuid::Uuid::new_v4().to_string(),
        email: req.email.clone(),
        invited_by: viewer.user_id,
        created_at: Utc::now(),
        accepted_at: None,
    };

    match state.store.create_invite(&invite).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id": invite.id, "email": invite.email})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn list_invites(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin_viewer(&state, &headers).await {
        return e;
    }

    match state.store.list_invites().await {
        Ok(invites) => (StatusCode::OK, Json(serde_json::json!({"invites": invites}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

fn urlencoded(s: &str) -> String {
    s.replace(':', "%3A").replace('/', "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_with_tokens(tokens: &[&str]) -> AuthConfig {
        AuthConfig {
            google_client_id: String::new(),
            google_client_secret: String::new(),
            google_redirect_uri: String::new(),
            jwt_secret: "test-secret-0123456789abcdef0123456789abcdef".into(),
            admin_emails: vec![],
            api_tokens: tokens.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn headers_with_bearer(token: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        h
    }

    #[test]
    fn api_token_matches_any_configured_token_exactly() {
        let auth = auth_with_tokens(&["tok-a", "tok-b"]);
        assert!(matches_api_token(&auth, &headers_with_bearer("tok-b")));
        assert!(matches_api_token(&auth, &headers_with_bearer("tok-a")));
    }

    #[test]
    fn api_token_rejects_wrong_or_mangled_values() {
        let auth = auth_with_tokens(&["tok-a"]);
        assert!(!matches_api_token(&auth, &headers_with_bearer("tok-b")));
        assert!(!matches_api_token(
            &auth,
            &headers_with_bearer("\"tok-a\"")
        ));
        assert!(!matches_api_token(&auth, &headers_with_bearer("tok-a ")));
        assert!(!matches_api_token(&auth, &HeaderMap::new()));
    }
}
