use axum::{Json, Router, extract::State, routing::get};
use validator::Validate;

use crate::AppState;
use crate::shared::errors::AppError;
use crate::shared::extractors::AdminUser;

use super::model::*;
use super::service;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_categories).post(create_category))
}

/// GET /api/categories (public)
async fn list_categories(State(state): State<AppState>) -> Result<Json<Vec<Category>>, AppError> {
    let categories = service::list_categories(&state.pool).await?;
    Ok(Json(categories))
}

/// POST /api/categories (admin only)
async fn create_category(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(body): Json<CreateCategoryRequest>,
) -> Result<Json<Category>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let category = service::create_category(&state.pool, &body).await?;
    Ok(Json(category))
}
