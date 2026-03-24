use axum::{extract::State, routing::get, Json, Router};

use crate::shared::errors::AppError;
use crate::shared::extractors::AuthUser;
use crate::AppState;

use super::model::*;
use super::service;

pub fn router() -> Router<AppState> {
    Router::new().route("/me", get(get_profile).put(update_profile))
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
async fn update_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<UpdateProfileRequest>,
) -> Result<Json<UserProfileResponse>, AppError> {
    let user = if let Some(email) = body.email {
        service::update_user_email(&state.pool, auth.user_id, &email).await?
    } else {
        service::get_user_by_id(&state.pool, auth.user_id).await?
    };

    Ok(Json(UserProfileResponse::from(user)))
}
