use axum::{
    extract::State,
    http::header::{HeaderMap, SET_COOKIE},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use axum_extra::extract::CookieJar;
use validator::Validate;

use crate::shared::email;
use crate::shared::errors::AppError;
use crate::shared::extractors::AuthUser;
use crate::AppState;

use super::model::*;
use super::service;

/// Cookie name for refresh token
const REFRESH_TOKEN_COOKIE: &str = "refresh_token";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/verify-email", post(verify_email))
        .route("/login", post(login))
        .route("/google/login", post(google_login))
        .route("/github/login", post(github_login))
        .route("/refresh", post(refresh_token))
        .route("/logout", post(logout))
}

/// Build a Set-Cookie header string for the refresh token (HttpOnly, Secure, SameSite)
fn build_refresh_cookie(token: &str, max_age_days: i64) -> String {
    let max_age_seconds = max_age_days * 24 * 60 * 60;
    format!(
        "{}={}; HttpOnly; Secure; SameSite=Strict; Path=/api/auth; Max-Age={}",
        REFRESH_TOKEN_COOKIE, token, max_age_seconds
    )
}

/// Build a Set-Cookie header that clears the refresh token cookie
fn build_clear_refresh_cookie() -> String {
    format!(
        "{}=; HttpOnly; Secure; SameSite=Strict; Path=/api/auth; Max-Age=0",
        REFRESH_TOKEN_COOKIE
    )
}

/// POST /api/auth/register
/// Sends verification email (does NOT create user yet)
async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let code = service::create_email_verification(&state.pool, &body.email, &body.password).await?;

    // Send verification email (fire and forget in background)
    let config = state.config.clone();
    let to = body.email.clone();
    tokio::spawn(async move {
        if let Err(e) = email::send_verification_email(&to, &code, &config).await {
            tracing::error!("Failed to send verification email: {}", e);
        }
    });

    Ok(Json(MessageResponse {
        message: "Verification code sent to your email".to_string(),
    }))
}

/// POST /api/auth/verify-email
/// Verifies email code and creates user account (must login separately for tokens)
async fn verify_email(
    State(state): State<AppState>,
    Json(body): Json<VerifyEmailRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let response =
        service::verify_email_and_create_user(&state.pool, &body.email, &body.code).await?;

    // Send welcome email in background
    let config = state.config.clone();
    let to = body.email.clone();
    tokio::spawn(async move {
        if let Err(e) = email::send_welcome_email(&to, &config).await {
            tracing::error!("Failed to send welcome email: {}", e);
        }
    });

    Ok(Json(response))
}

/// POST /api/auth/login
/// Authenticates user and returns access_token in body + refresh_token as HttpOnly cookie
async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let tokens = service::login(&state.pool, &body.email, &body.password, &state.config).await?;

    // Send login notification in background
    let config = state.config.clone();
    let to = body.email.clone();
    tokio::spawn(async move {
        if let Err(e) = email::send_login_notification(&to, &config).await {
            tracing::error!("Failed to send login notification: {}", e);
        }
    });

    // Set refresh token as HttpOnly cookie
    let cookie = build_refresh_cookie(&tokens.refresh_token, state.config.jwt_refresh_expiry_days);
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());

    let body = AuthResponse {
        access_token: tokens.access_token,
        user: tokens.user,
    };

    Ok((headers, Json(body)))
}

/// POST /api/auth/google/login
/// Authenticates user with Google ID token and returns tokens
async fn google_login(
    State(state): State<AppState>,
    Json(body): Json<GoogleLoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let tokens = service::google_login(&state.pool, &body.token, &state.config).await?;

    // Set refresh token as HttpOnly cookie
    let cookie = build_refresh_cookie(&tokens.refresh_token, state.config.jwt_refresh_expiry_days);
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());

    let body = AuthResponse {
        access_token: tokens.access_token,
        user: tokens.user,
    };

    Ok((headers, Json(body)))
}

/// POST /api/auth/github/login
/// Authenticates user with GitHub authorization code and returns tokens
async fn github_login(
    State(state): State<AppState>,
    Json(body): Json<GithubLoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let tokens = service::github_login(&state.pool, &body.code, &state.config).await?;

    // Set refresh token as HttpOnly cookie
    let cookie = build_refresh_cookie(&tokens.refresh_token, state.config.jwt_refresh_expiry_days);
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());

    let body = AuthResponse {
        access_token: tokens.access_token,
        user: tokens.user,
    };

    Ok((headers, Json(body)))
}

/// POST /api/auth/refresh
/// Reads refresh_token from cookie, rotates it, returns new access_token + new cookie
async fn refresh_token(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let old_token = jar
        .get(REFRESH_TOKEN_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| AppError::Unauthorized("No refresh token cookie".to_string()))?;

    let tokens = service::rotate_refresh_token(&state.pool, &old_token, &state.config).await?;

    // Set new refresh token cookie
    let cookie = build_refresh_cookie(&tokens.refresh_token, state.config.jwt_refresh_expiry_days);
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());

    let body = AuthResponse {
        access_token: tokens.access_token,
        user: tokens.user,
    };

    Ok((headers, Json(body)))
}

/// POST /api/auth/logout
/// Clears refresh token from DB and cookie
async fn logout(
    State(state): State<AppState>,
    _auth: AuthUser,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    // Try to delete from DB if cookie exists
    if let Some(cookie) = jar.get(REFRESH_TOKEN_COOKIE) {
        service::logout(&state.pool, cookie.value()).await?;
    }

    // Clear the cookie
    let clear_cookie = build_clear_refresh_cookie();
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, clear_cookie.parse().unwrap());

    let body = MessageResponse {
        message: "Logged out successfully".to_string(),
    };

    Ok((headers, Json(body)))
}
