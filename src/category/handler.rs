use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use uuid::Uuid;
use validator::Validate;

use crate::AppState;
use crate::shared::errors::AppError;
use crate::shared::extractors::AdminUser;

use super::model::*;
use super::service;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_categories).post(create_category))
        .route(
            "/{id}",
            get(get_category)
                .put(update_category)
                .delete(delete_category),
        )
}

/// GET /api/categories (public)
async fn list_categories(State(state): State<AppState>) -> Result<Json<Vec<Category>>, AppError> {
    let categories = service::list_categories(&state.pool).await?;
    Ok(Json(categories))
}

/// GET /api/categories/:id (public)
async fn get_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Category>, AppError> {
    let category = service::get_category(&state.pool, id).await?;
    Ok(Json(category))
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

/// PUT /api/categories/:id (admin only)
async fn update_category(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateCategoryRequest>,
) -> Result<Json<Category>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let category = service::update_category(&state.pool, id, &body).await?;
    Ok(Json(category))
}

/// DELETE /api/categories/:id (admin only)
async fn delete_category(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    service::delete_category(&state.pool, id).await?;
    Ok(Json(
        serde_json::json!({ "message": "Category deleted successfully" }),
    ))
}
