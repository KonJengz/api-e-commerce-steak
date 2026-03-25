use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::header::{HeaderMap, SET_COOKIE},
    response::IntoResponse,
    routing::post,
};
use axum_extra::extract::CookieJar;
use validator::Validate;

use crate::AppState;
use crate::shared::email;
use crate::shared::errors::AppError;
use crate::shared::extractors::AuthUser;
use crate::shared::http::client_ip;
use crate::shared::rate_limit::RateLimitRule;

use super::model::*;
use super::service;

/// Cookie name for refresh token
const REFRESH_TOKEN_COOKIE: &str = "refresh_token";
const AUTH_RATE_LIMIT_WINDOW_SECONDS: u64 = 15 * 60;

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

/// Build a Set-Cookie header string for the refresh token (HttpOnly, SameSite)
fn build_refresh_cookie(token: &str, max_age_days: i64, cookie_secure: bool) -> String {
    let max_age_seconds = max_age_days * 24 * 60 * 60;
    let secure_flag = if cookie_secure { "Secure; " } else { "" };
    format!(
        "{}={}; HttpOnly; {}SameSite=Strict; Path=/api/auth; Max-Age={}",
        REFRESH_TOKEN_COOKIE, token, secure_flag, max_age_seconds
    )
}

/// Build a Set-Cookie header that clears the refresh token cookie
fn build_clear_refresh_cookie(cookie_secure: bool) -> String {
    let secure_flag = if cookie_secure { "Secure; " } else { "" };
    format!(
        "{}=; HttpOnly; {}SameSite=Strict; Path=/api/auth; Max-Age=0",
        REFRESH_TOKEN_COOKIE, secure_flag
    )
}

async fn apply_rate_limit(
    state: &AppState,
    key: String,
    scope: &'static str,
    max_attempts: u32,
) -> Result<(), AppError> {
    state
        .auth_rate_limiter
        .check(
            key,
            RateLimitRule {
                max_attempts,
                window: Duration::from_secs(AUTH_RATE_LIMIT_WINDOW_SECONDS),
                scope,
            },
        )
        .await
}

async fn apply_ip_limit(
    state: &AppState,
    scope: &'static str,
    client_ip: &str,
    max_attempts: u32,
) -> Result<(), AppError> {
    apply_rate_limit(
        state,
        format!("auth:{}:ip:{}", scope, client_ip),
        scope,
        max_attempts,
    )
    .await
}

async fn apply_ip_and_email_limit(
    state: &AppState,
    scope: &'static str,
    client_ip: &str,
    email: &str,
    ip_max_attempts: u32,
    email_max_attempts: u32,
) -> Result<(), AppError> {
    apply_ip_limit(state, scope, client_ip, ip_max_attempts).await?;
    apply_rate_limit(
        state,
        format!("auth:{}:email:{}", scope, email.trim().to_ascii_lowercase()),
        scope,
        email_max_attempts,
    )
    .await
}

/// POST /api/auth/register
/// Sends verification email (does NOT create user yet)
async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<RegisterRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_and_email_limit(&state, "register", &client_ip, &body.email, 10, 3).await?;

    let code =
        service::create_email_verification(&state.pool, &body.email, &body.password, &state.config)
            .await?;

    if let Err(send_error) = email::send_verification_email(&body.email, &code, &state.config).await
    {
        if let Err(cleanup_error) =
            service::clear_pending_email_verification(&state.pool, &body.email).await
        {
            tracing::error!(
                email = %body.email,
                error = %cleanup_error,
                "failed to clean up pending registration verification after email send failure"
            );
        }

        return Err(send_error);
    }

    Ok(Json(MessageResponse {
        message: "Verification code sent to your email".to_string(),
    }))
}

/// POST /api/auth/verify-email
/// Verifies email code and creates user account (must login separately for tokens)
async fn verify_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<VerifyEmailRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_and_email_limit(&state, "verify_email", &client_ip, &body.email, 20, 5).await?;

    let response =
        service::verify_email_and_create_user(&state.pool, &body.email, &body.code, &state.config)
            .await?;

    Ok(Json(response))
}

/// POST /api/auth/login
/// Authenticates user and returns access_token in body + refresh_token as HttpOnly cookie
async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_and_email_limit(&state, "login", &client_ip, &body.email, 20, 10).await?;

    let tokens = service::login(&state.pool, &body.email, &body.password, &state.config).await?;

    // Set refresh token as HttpOnly cookie
    let cookie = build_refresh_cookie(
        &tokens.refresh_token,
        state.config.jwt_refresh_expiry_days,
        state.config.cookie_secure,
    );
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
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<GoogleLoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_limit(&state, "google_login", &client_ip, 20).await?;

    let tokens = service::google_login(&state.pool, &body.token, &state.config).await?;

    // Set refresh token as HttpOnly cookie
    let cookie = build_refresh_cookie(
        &tokens.refresh_token,
        state.config.jwt_refresh_expiry_days,
        state.config.cookie_secure,
    );
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
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<GithubLoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_limit(&state, "github_login", &client_ip, 20).await?;

    let tokens = service::github_login(&state.pool, &body.code, &state.config).await?;

    // Set refresh token as HttpOnly cookie
    let cookie = build_refresh_cookie(
        &tokens.refresh_token,
        state.config.jwt_refresh_expiry_days,
        state.config.cookie_secure,
    );
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
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_limit(&state, "refresh", &client_ip, 30).await?;

    let old_token = jar
        .get(REFRESH_TOKEN_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| AppError::Unauthorized("No refresh token cookie".to_string()))?;

    let tokens = service::rotate_refresh_token(&state.pool, &old_token, &state.config).await?;

    // Set new refresh token cookie
    let cookie = build_refresh_cookie(
        &tokens.refresh_token,
        state.config.jwt_refresh_expiry_days,
        state.config.cookie_secure,
    );
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
        service::logout(&state.pool, cookie.value(), &state.config).await?;
    }

    // Clear the cookie
    let clear_cookie = build_clear_refresh_cookie(state.config.cookie_secure);
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, clear_cookie.parse().unwrap());

    let body = MessageResponse {
        message: "Logged out successfully".to_string(),
    };

    Ok((headers, Json(body)))
}
