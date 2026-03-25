use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::HeaderMap,
    routing::{get, post},
};
use validator::Validate;

use crate::AppState;
use crate::auth::model::MessageResponse;
use crate::shared::email;
use crate::shared::errors::AppError;
use crate::shared::extractors::AuthUser;
use crate::shared::http::client_ip;
use crate::shared::rate_limit::RateLimitRule;

use super::model::*;
use super::service;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(get_profile).put(request_email_change))
        .route("/me/verify-email-change", post(verify_email_change))
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
                window: Duration::from_secs(15 * 60),
                scope,
            },
        )
        .await
}

/// GET /api/users/me
async fn get_profile(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<UserProfileResponse>, AppError> {
    let user = service::get_user_by_id(&state.pool, auth.user_id).await?;
    Ok(Json(UserProfileResponse::from(user)))
}

/// PUT /api/users/me
async fn request_email_change(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<RequestEmailChangeRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_rate_limit(
        &state,
        format!(
            "user:request_email_change:{}:{}",
            client_ip,
            body.email.trim().to_ascii_lowercase()
        ),
        "email_change",
        3,
    )
    .await?;

    let code = service::request_email_change(&state.pool, auth.user_id, &body.email, &state.config)
        .await?;

    if let Err(send_error) = email::send_verification_email(&body.email, &code, &state.config).await
    {
        if let Err(cleanup_error) =
            service::clear_pending_email_change_verification(&state.pool, auth.user_id, &body.email)
                .await
        {
            tracing::error!(
                user_id = %auth.user_id,
                email = %body.email,
                error = %cleanup_error,
                "failed to clean up pending email-change verification after email send failure"
            );
        }

        return Err(send_error);
    }

    Ok(Json(MessageResponse {
        message: "Verification code sent to your new email address".to_string(),
    }))
}

async fn verify_email_change(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<VerifyEmailChangeRequest>,
) -> Result<Json<UserProfileResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_rate_limit(
        &state,
        format!(
            "user:verify_email_change:{}:{}",
            client_ip,
            body.email.trim().to_ascii_lowercase()
        ),
        "email_change_verify",
        5,
    )
    .await?;

    let user = service::verify_email_change(
        &state.pool,
        auth.user_id,
        &body.email,
        &body.code,
        &state.config,
    )
    .await?;

    Ok(Json(UserProfileResponse::from(user)))
}
