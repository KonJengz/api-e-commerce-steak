use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{ConnectInfo, Query, State},
    http::header::{HeaderMap, SET_COOKIE},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use reqwest::Url;
use serde::Deserialize;
use validator::Validate;

use crate::AppState;
use crate::config::AppConfig;
use crate::shared::email;
use crate::shared::errors::AppError;
use crate::shared::extractors::AuthUser;
use crate::shared::http::client_ip;
use crate::shared::jwt;
use crate::shared::rate_limit::RateLimitRule;

use super::model::*;
use super::service;

const REFRESH_TOKEN_COOKIE: &str = "refresh_token";
const AUTH_RATE_LIMIT_WINDOW_SECONDS: u64 = 15 * 60;
const OAUTH_STATE_COOKIE: &str = "oauth_state";
const OAUTH_EXCHANGE_URL_COOKIE: &str = "oauth_exchange_url";
const OAUTH_REDIRECT_TO_COOKIE: &str = "oauth_redirect_to";
const OAUTH_COOKIE_PATH: &str = "/api/auth";

#[derive(Debug, Deserialize)]
struct OauthStartQuery {
    exchange_url: String,
    redirect_to: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OauthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Debug)]
struct OauthFlow {
    exchange_url: String,
    redirect_to: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/verify-email", post(verify_email))
        .route("/login", post(login))
        .route("/google/start", get(google_start))
        .route("/google/callback", get(google_callback))
        .route("/google/login", post(google_login))
        .route("/github/start", get(github_start))
        .route("/github/callback", get(github_callback))
        .route("/github/login", post(github_login))
        .route("/oauth/exchange", post(oauth_exchange))
        .route("/refresh", post(refresh_token))
        .route("/logout", post(logout))
}

fn build_refresh_cookie(token: &str, max_age_days: i64, cookie_secure: bool) -> String {
    let max_age_seconds = max_age_days * 24 * 60 * 60;
    let secure_flag = if cookie_secure { "Secure; " } else { "" };
    format!(
        "{}={}; HttpOnly; {}SameSite=Strict; Path=/api/auth; Max-Age={}",
        REFRESH_TOKEN_COOKIE, token, secure_flag, max_age_seconds
    )
}

fn build_clear_refresh_cookie(cookie_secure: bool) -> String {
    let secure_flag = if cookie_secure { "Secure; " } else { "" };
    format!(
        "{}=; HttpOnly; {}SameSite=Strict; Path=/api/auth; Max-Age=0",
        REFRESH_TOKEN_COOKIE, secure_flag
    )
}

fn oauth_cookie(name: &'static str, value: String, cookie_secure: bool) -> Cookie<'static> {
    Cookie::build((name, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(cookie_secure)
        .path(OAUTH_COOKIE_PATH)
        .build()
}

fn oauth_cookie_tombstone(name: &'static str, cookie_secure: bool) -> Cookie<'static> {
    Cookie::build((name, ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(cookie_secure)
        .path(OAUTH_COOKIE_PATH)
        .build()
}

fn clear_oauth_flow_cookies(jar: CookieJar, cookie_secure: bool) -> CookieJar {
    jar.remove(oauth_cookie_tombstone(OAUTH_STATE_COOKIE, cookie_secure))
        .remove(oauth_cookie_tombstone(
            OAUTH_EXCHANGE_URL_COOKIE,
            cookie_secure,
        ))
        .remove(oauth_cookie_tombstone(
            OAUTH_REDIRECT_TO_COOKIE,
            cookie_secure,
        ))
}

fn normalize_redirect_to(redirect_to: Option<&str>) -> String {
    match redirect_to {
        Some(value) if value.starts_with('/') && !value.starts_with("//") => value.to_string(),
        _ => "/account".to_string(),
    }
}

fn is_google_oauth_configured(config: &AppConfig) -> bool {
    !config.google_client_id.contains("your-google-client-id")
        && !config
            .google_client_secret
            .contains("your-google-client-secret")
}

fn is_github_oauth_configured(config: &AppConfig) -> bool {
    !config.github_client_id.contains("your-github-client-id")
        && !config
            .github_client_secret
            .contains("your-github-client-secret")
}

fn frontend_login_redirect(config: &AppConfig, error_code: &str) -> Redirect {
    let mut url = Url::parse(&config.app_url).expect("APP_URL must be a valid URL");
    url.set_path("/login");
    url.query_pairs_mut().append_pair("oauth_error", error_code);
    Redirect::to(url.as_ref())
}

fn validate_exchange_url(exchange_url: &str, config: &AppConfig) -> Result<String, AppError> {
    let parsed = Url::parse(exchange_url)
        .map_err(|_| AppError::BadRequest("Invalid exchange callback URL".to_string()))?;
    let frontend = Url::parse(&config.app_url)
        .map_err(|_| AppError::Internal("APP_URL must be a valid URL".to_string()))?;

    if parsed.origin() != frontend.origin() {
        return Err(AppError::BadRequest(
            "Exchange callback must use the configured frontend origin".to_string(),
        ));
    }

    if parsed.path() != "/login/oauth/callback" {
        return Err(AppError::BadRequest(
            "Exchange callback path must be /login/oauth/callback".to_string(),
        ));
    }

    Ok(parsed.to_string())
}

fn backend_origin(headers: &HeaderMap, trust_proxy_headers: bool) -> Result<String, AppError> {
    let host = if trust_proxy_headers {
        headers
            .get("x-forwarded-host")
            .or_else(|| headers.get("host"))
    } else {
        headers.get("host")
    }
    .and_then(|value| value.to_str().ok())
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| AppError::Internal("Missing request host header".to_string()))?;

    let protocol = if trust_proxy_headers {
        headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("http")
    } else {
        "http"
    };

    Ok(format!("{}://{}", protocol, host))
}

fn provider_callback_url(origin: &str, path: &str) -> String {
    format!("{}{}", origin.trim_end_matches('/'), path)
}

fn oauth_flow_from_jar(jar: &CookieJar, state: Option<&str>) -> Result<OauthFlow, &'static str> {
    let expected_state = jar
        .get(OAUTH_STATE_COOKIE)
        .map(|cookie| cookie.value().to_string());
    let exchange_url = jar
        .get(OAUTH_EXCHANGE_URL_COOKIE)
        .map(|cookie| cookie.value().to_string());
    let redirect_to = normalize_redirect_to(
        jar.get(OAUTH_REDIRECT_TO_COOKIE)
            .map(|cookie| cookie.value()),
    );

    if expected_state.as_deref().is_none() || state.is_none() || expected_state.as_deref() != state
    {
        return Err("invalid_oauth_state");
    }

    let exchange_url = exchange_url.ok_or("missing_oauth_callback")?;

    Ok(OauthFlow {
        exchange_url,
        redirect_to,
    })
}

fn oauth_success_redirect(
    exchange_url: &str,
    ticket: &str,
    redirect_to: &str,
) -> Result<Redirect, AppError> {
    let mut url = Url::parse(exchange_url)
        .map_err(|_| AppError::Internal("Invalid exchange callback URL".to_string()))?;
    url.query_pairs_mut()
        .append_pair("ticket", ticket)
        .append_pair("redirectTo", redirect_to);

    Ok(Redirect::to(url.as_ref()))
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

async fn google_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<OauthStartQuery>,
) -> Result<(CookieJar, Redirect), AppError> {
    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_limit(&state, "google_oauth_start", &client_ip, 20).await?;

    if !is_google_oauth_configured(&state.config) {
        return Ok((
            CookieJar::new(),
            frontend_login_redirect(&state.config, "google_not_configured"),
        ));
    }

    let exchange_url = validate_exchange_url(&query.exchange_url, &state.config)?;
    let redirect_to = normalize_redirect_to(query.redirect_to.as_deref());
    let origin = backend_origin(&headers, state.config.trust_proxy_headers)?;
    let callback_url = provider_callback_url(&origin, "/api/auth/google/callback");
    let state_token = jwt::create_refresh_token();

    let mut authorize_url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
        .map_err(|_| AppError::Internal("Failed to build Google authorize URL".to_string()))?;
    authorize_url
        .query_pairs_mut()
        .append_pair("client_id", &state.config.google_client_id)
        .append_pair("redirect_uri", &callback_url)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("prompt", "select_account")
        .append_pair("state", &state_token);

    let jar = CookieJar::new()
        .add(oauth_cookie(
            OAUTH_STATE_COOKIE,
            state_token,
            state.config.cookie_secure,
        ))
        .add(oauth_cookie(
            OAUTH_EXCHANGE_URL_COOKIE,
            exchange_url,
            state.config.cookie_secure,
        ))
        .add(oauth_cookie(
            OAUTH_REDIRECT_TO_COOKIE,
            redirect_to,
            state.config.cookie_secure,
        ));

    Ok((jar, Redirect::to(authorize_url.as_ref())))
}

async fn google_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Query(query): Query<OauthCallbackQuery>,
) -> Result<(CookieJar, Redirect), AppError> {
    let flow = oauth_flow_from_jar(&jar, query.state.as_deref());
    let cleared_jar = clear_oauth_flow_cookies(jar, state.config.cookie_secure);

    let flow = match flow {
        Ok(flow) => flow,
        Err(error_code) => {
            return Ok((
                cleared_jar,
                frontend_login_redirect(&state.config, error_code),
            ));
        }
    };

    if query.error.is_some() {
        return Ok((
            cleared_jar,
            frontend_login_redirect(&state.config, "google_access_denied"),
        ));
    }

    let code = match query.code.as_deref() {
        Some(code) => code,
        None => {
            return Ok((
                cleared_jar,
                frontend_login_redirect(&state.config, "missing_oauth_code"),
            ));
        }
    };

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_limit(&state, "google_oauth_callback", &client_ip, 20).await?;

    let origin = backend_origin(&headers, state.config.trust_proxy_headers)?;
    let callback_url = provider_callback_url(&origin, "/api/auth/google/callback");

    let tokens = match service::google_login_with_authorization_code(
        &state.pool,
        code,
        &callback_url,
        &state.config,
    )
    .await
    {
        Ok(tokens) => tokens,
        Err(_) => {
            return Ok((
                cleared_jar,
                frontend_login_redirect(&state.config, "google_sign_in_failed"),
            ));
        }
    };

    let ticket = match service::create_oauth_login_ticket(&state.pool, &tokens).await {
        Ok(ticket) => ticket,
        Err(_) => {
            return Ok((
                cleared_jar,
                frontend_login_redirect(&state.config, "oauth_ticket_failed"),
            ));
        }
    };

    let redirect = oauth_success_redirect(&flow.exchange_url, &ticket, &flow.redirect_to)?;
    Ok((cleared_jar, redirect))
}

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

async fn github_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<OauthStartQuery>,
) -> Result<(CookieJar, Redirect), AppError> {
    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_limit(&state, "github_oauth_start", &client_ip, 20).await?;

    if !is_github_oauth_configured(&state.config) {
        return Ok((
            CookieJar::new(),
            frontend_login_redirect(&state.config, "github_not_configured"),
        ));
    }

    let exchange_url = validate_exchange_url(&query.exchange_url, &state.config)?;
    let redirect_to = normalize_redirect_to(query.redirect_to.as_deref());
    let origin = backend_origin(&headers, state.config.trust_proxy_headers)?;
    let callback_url = provider_callback_url(&origin, "/api/auth/github/callback");
    let state_token = jwt::create_refresh_token();

    let mut authorize_url = Url::parse("https://github.com/login/oauth/authorize")
        .map_err(|_| AppError::Internal("Failed to build GitHub authorize URL".to_string()))?;
    authorize_url
        .query_pairs_mut()
        .append_pair("client_id", &state.config.github_client_id)
        .append_pair("redirect_uri", &callback_url)
        .append_pair("scope", "read:user user:email")
        .append_pair("state", &state_token);

    let jar = CookieJar::new()
        .add(oauth_cookie(
            OAUTH_STATE_COOKIE,
            state_token,
            state.config.cookie_secure,
        ))
        .add(oauth_cookie(
            OAUTH_EXCHANGE_URL_COOKIE,
            exchange_url,
            state.config.cookie_secure,
        ))
        .add(oauth_cookie(
            OAUTH_REDIRECT_TO_COOKIE,
            redirect_to,
            state.config.cookie_secure,
        ));

    Ok((jar, Redirect::to(authorize_url.as_ref())))
}

async fn github_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Query(query): Query<OauthCallbackQuery>,
) -> Result<(CookieJar, Redirect), AppError> {
    let flow = oauth_flow_from_jar(&jar, query.state.as_deref());
    let cleared_jar = clear_oauth_flow_cookies(jar, state.config.cookie_secure);

    let flow = match flow {
        Ok(flow) => flow,
        Err(error_code) => {
            return Ok((
                cleared_jar,
                frontend_login_redirect(&state.config, error_code),
            ));
        }
    };

    if query.error.is_some() {
        return Ok((
            cleared_jar,
            frontend_login_redirect(&state.config, "github_access_denied"),
        ));
    }

    let code = match query.code.as_deref() {
        Some(code) => code,
        None => {
            return Ok((
                cleared_jar,
                frontend_login_redirect(&state.config, "missing_oauth_code"),
            ));
        }
    };

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_limit(&state, "github_oauth_callback", &client_ip, 20).await?;

    let tokens = match service::github_login(&state.pool, code, &state.config).await {
        Ok(tokens) => tokens,
        Err(_) => {
            return Ok((
                cleared_jar,
                frontend_login_redirect(&state.config, "github_sign_in_failed"),
            ));
        }
    };

    let ticket = match service::create_oauth_login_ticket(&state.pool, &tokens).await {
        Ok(ticket) => ticket,
        Err(_) => {
            return Ok((
                cleared_jar,
                frontend_login_redirect(&state.config, "oauth_ticket_failed"),
            ));
        }
    };

    let redirect = oauth_success_redirect(&flow.exchange_url, &ticket, &flow.redirect_to)?;
    Ok((cleared_jar, redirect))
}

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

async fn oauth_exchange(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<OauthExchangeRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_ip_limit(&state, "oauth_exchange", &client_ip, 20).await?;

    let tokens = service::exchange_oauth_login_ticket(&state.pool, &body.ticket).await?;
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

async fn logout(
    State(state): State<AppState>,
    _auth: AuthUser,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    if let Some(cookie) = jar.get(REFRESH_TOKEN_COOKIE) {
        service::logout(&state.pool, cookie.value(), &state.config).await?;
    }

    let clear_cookie = build_clear_refresh_cookie(state.config.cookie_secure);
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, clear_cookie.parse().unwrap());

    let body = MessageResponse {
        message: "Logged out successfully".to_string(),
    };

    Ok((headers, Json(body)))
}
