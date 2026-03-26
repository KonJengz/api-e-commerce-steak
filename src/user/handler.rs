use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Multipart, State},
    http::{HeaderMap, header::SET_COOKIE},
    response::IntoResponse,
    routing::{get, post, put},
};
use validator::Validate;

use crate::AppState;
use crate::auth::model::MessageResponse;
use crate::auth::{model as auth_model, service as auth_service};
use crate::shared::cloudinary;
use crate::shared::email;
use crate::shared::errors::AppError;
use crate::shared::extractors::AuthUser;
use crate::shared::http::client_ip;
use crate::shared::rate_limit::RateLimitRule;

use super::model::*;
use super::service;

const MAX_IMAGE_SIZE_BYTES: usize = 5 * 1024 * 1024;
const MAX_MULTIPART_BODY_SIZE_BYTES: usize = 6 * 1024 * 1024;
const ALLOWED_IMAGE_CONTENT_TYPES: [&str; 3] = ["image/jpeg", "image/png", "image/webp"];
const REFRESH_TOKEN_COOKIE: &str = "refresh_token";

#[derive(Debug)]
struct UploadedProfileImage {
    file_name: String,
    content_type: String,
    file_data: Vec<u8>,
}

pub fn router() -> Router<AppState> {
    let profile_router = Router::new()
        .route("/me/profile", put(update_profile))
        .layer(DefaultBodyLimit::max(MAX_MULTIPART_BODY_SIZE_BYTES));

    Router::new()
        .route("/me", get(get_profile).put(request_email_change))
        .route("/me/password", put(change_password))
        .route("/me/set-password", post(set_password))
        .route("/me/verify-email-change", post(verify_email_change))
        .merge(profile_router)
}

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

/// PUT /api/users/me/profile
async fn update_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<UserProfileResponse>, AppError> {
    let mut name: Option<String> = None;
    let mut remove_image = false;
    let mut image: Option<UploadedProfileImage> = None;

    loop {
        let Some(field) = multipart.next_field().await.map_err(map_multipart_error)? else {
            break;
        };

        match field.name() {
            Some("name") => {
                name = Some(field.text().await.map_err(map_multipart_error)?);
            }
            Some("remove_image") => {
                let value = field.text().await.map_err(map_multipart_error)?;
                remove_image = parse_bool_form_field(&value)?;
            }
            Some("image") => {
                let file_name = field.file_name().unwrap_or("avatar.jpg").to_string();
                let content_type = field.content_type().unwrap_or("image/jpeg").to_string();
                let file_data = field.bytes().await.map_err(map_multipart_error)?.to_vec();

                validate_uploaded_image(&file_data, &content_type)?;

                image = Some(UploadedProfileImage {
                    file_name,
                    content_type,
                    file_data,
                });
            }
            _ => {
                let _ = field.bytes().await.map_err(map_multipart_error)?;
            }
        }
    }

    if name.is_none() && image.is_none() && !remove_image {
        return Err(AppError::BadRequest(
            "Provide at least one of `name`, `image`, or `remove_image=true`".to_string(),
        ));
    }

    if remove_image && image.is_some() {
        return Err(AppError::BadRequest(
            "Cannot upload a new image and remove the current image in the same request"
                .to_string(),
        ));
    }

    let uploaded_image = match image {
        Some(image) => Some(
            cloudinary::upload_user_image(
                &image.file_name,
                image.file_data,
                &image.content_type,
                &state.config,
            )
            .await?,
        ),
        None => None,
    };

    let image_update = match uploaded_image.as_ref() {
        Some(uploaded_image) => ProfileImageUpdate::Replace {
            image_url: &uploaded_image.secure_url,
            image_public_id: &uploaded_image.public_id,
        },
        None if remove_image => ProfileImageUpdate::Remove,
        None => ProfileImageUpdate::Keep,
    };

    let update_result =
        service::update_profile(&state.pool, auth.user_id, name.as_deref(), image_update).await;

    let (user, previous_image_public_id) = match update_result {
        Ok(result) => result,
        Err(error) => {
            if let Some(uploaded_image) = uploaded_image.as_ref()
                && let Err(cleanup_error) =
                    cloudinary::delete_image(&uploaded_image.public_id, &state.config).await
            {
                tracing::error!(
                    error = %cleanup_error,
                    user_id = %auth.user_id,
                    public_id = %uploaded_image.public_id,
                    "failed to roll back uploaded profile image after database error"
                );
            }

            return Err(error);
        }
    };

    if let Some(previous_image_public_id) = previous_image_public_id.as_deref()
        && let Err(error) = cloudinary::delete_image(previous_image_public_id, &state.config).await
    {
        tracing::error!(
            error = %error,
            user_id = %auth.user_id,
            public_id = %previous_image_public_id,
            "failed to delete replaced profile image from Cloudinary"
        );
    }

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

async fn change_password(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<auth_model::ChangePasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_rate_limit(
        &state,
        format!("user:change_password:{}:{}", client_ip, auth.user_id),
        "change_password",
        10,
    )
    .await?;

    auth_service::change_password(
        &state.pool,
        auth.user_id,
        &body.current_password,
        &body.new_password,
    )
    .await?;

    let clear_cookie = build_clear_refresh_cookie(state.config.cookie_secure);
    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, clear_cookie.parse().unwrap());

    Ok((
        response_headers,
        Json(MessageResponse {
            message: "Password changed successfully. Please log in again.".to_string(),
        }),
    ))
}

async fn set_password(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<auth_model::SetPasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let client_ip = client_ip(&headers, addr, state.config.trust_proxy_headers);
    apply_rate_limit(
        &state,
        format!("user:set_password:{}:{}", client_ip, auth.user_id),
        "set_password",
        10,
    )
    .await?;

    auth_service::set_password(&state.pool, auth.user_id, &body.new_password).await?;

    let clear_cookie = build_clear_refresh_cookie(state.config.cookie_secure);
    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, clear_cookie.parse().unwrap());

    Ok((
        response_headers,
        Json(MessageResponse {
            message: "Password set successfully. Please log in again.".to_string(),
        }),
    ))
}

fn validate_uploaded_image(file_data: &[u8], content_type: &str) -> Result<(), AppError> {
    if file_data.is_empty() {
        return Err(AppError::BadRequest("No image file uploaded".to_string()));
    }

    if file_data.len() > MAX_IMAGE_SIZE_BYTES {
        return Err(AppError::BadRequest(format!(
            "Image is too large. Maximum size is {} MB.",
            MAX_IMAGE_SIZE_BYTES / 1024 / 1024
        )));
    }

    if !ALLOWED_IMAGE_CONTENT_TYPES.contains(&content_type) {
        return Err(AppError::BadRequest(
            "Unsupported image type. Allowed types: image/jpeg, image/png, image/webp".to_string(),
        ));
    }

    Ok(())
}

fn parse_bool_form_field(value: &str) -> Result<bool, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" | "" => Ok(false),
        _ => Err(AppError::BadRequest(
            "remove_image must be a boolean value".to_string(),
        )),
    }
}

fn map_multipart_error(error: axum::extract::multipart::MultipartError) -> AppError {
    let message = error.to_string();

    if message.contains("length limit exceeded") || message.contains("body too large") {
        return AppError::BadRequest(format!(
            "Image upload is too large. Maximum request size is {} MB.",
            MAX_MULTIPART_BODY_SIZE_BYTES / 1024 / 1024
        ));
    }

    AppError::BadRequest(
        "Failed to parse multipart/form-data request. Make sure you send a FormData body with optional `name`, `remove_image`, and `image` fields.".to_string(),
    )
}
