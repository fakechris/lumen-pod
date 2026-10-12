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
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let admin_emails: Vec<String> = std::env::var("ADMIN_EMAILS")
            .unwrap_or_else(|_| "fakechris@gmail.com".into())
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        Self {
            google_client_id: std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default(),
            google_client_secret: std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default(),
            google_redirect_uri: std::env::var("GOOGLE_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:3210/auth/google/callback".into()),
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-secret-change-in-production".into()),
            admin_emails,
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

/// Require authenticated + approved user. Returns claims or error response.
pub fn require_approved(headers: &HeaderMap, jwt_secret: &str) -> Result<Claims, axum::response::Response> {
    match extract_claims(headers, jwt_secret) {
        None => Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "not authenticated"})),
        )
            .into_response()),
        Some(c) if !c.approved => Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "pending_approval",
                "message": "Your account is awaiting admin approval.",
                "email": c.email,
            })),
        )
            .into_response()),
        Some(c) => Ok(c),
    }
}

/// Require authenticated + approved + admin role.
pub fn require_admin(headers: &HeaderMap, jwt_secret: &str) -> Result<Claims, axum::response::Response> {
    let claims = require_approved(headers, jwt_secret)?;
    if claims.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "admin only"})),
        )
            .into_response());
    }
    Ok(claims)
}

// ---- Google OAuth Handlers ----

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub auth: AuthConfig,
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

    let cookie = format!(
        "lumen_pod_session={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800",
        jwt
    );

    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({
            "status": "ok",
            "token": jwt,
            "user": {
                "id": user.id,
                "email": user.email,
                "name": user.name,
                "avatar_url": user.avatar_url,
                "role": role_str,
                "approved": user.approved,
            }
        })),
    )
        .into_response()
}

/// GET /api/me — always works for authenticated users (even pending)
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match extract_claims(&headers, &state.auth.jwt_secret) {
        Some(claims) => {
            let user = state.store.get_user_by_email(&claims.email).await;
            match user {
                Ok(Some(u)) => {
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
                        })),
                    )
                        .into_response()
                }
                _ => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "id": claims.sub,
                        "email": claims.email,
                        "role": claims.role,
                        "approved": claims.approved,
                    })),
                )
                    .into_response(),
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
    if let Err(e) = require_admin(&headers, &state.auth.jwt_secret) {
        return e;
    }

    match state.store.list_users().await {
        Ok(users) => {
            let data: Vec<_> = users
                .iter()
                .map(|u| {
                    serde_json::json!({
                        "id": u.id,
                        "email": u.email,
                        "name": u.name,
                        "avatar_url": u.avatar_url,
                        "role": match u.role { UserRole::Admin => "admin", UserRole::Member => "member" },
                        "approved": u.approved,
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
}

pub async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&headers, &state.auth.jwt_secret) {
        return e;
    }

    if let Some(true) = req.approved {
        let _ = state.store.approve_user(&user_id).await;
    }
    if let Some(role) = &req.role {
        if role == "admin" || role == "member" {
            let _ = state.store.set_user_role(&user_id, role).await;
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"updated": true}))).into_response()
}

pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&headers, &state.auth.jwt_secret) {
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
    let claims = match require_admin(&headers, &state.auth.jwt_secret) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let invite = lumen_pod_core::episode::Invite {
        id: uuid::Uuid::new_v4().to_string(),
        email: req.email.clone(),
        invited_by: claims.sub,
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
    if let Err(e) = require_admin(&headers, &state.auth.jwt_secret) {
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
