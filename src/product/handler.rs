use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use uuid::Uuid;
use validator::Validate;

use crate::shared::errors::AppError;
use crate::shared::extractors::AdminUser;
use crate::AppState;

use super::model::*;
use super::service;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_products).post(create_product))
        .route("/{id}", get(get_product).put(update_product).delete(delete_product))
}

/// GET /api/products (public)
async fn list_products(
    State(state): State<AppState>,
) -> Result<Json<Vec<Product>>, AppError> {
    let products = service::list_products(&state.pool).await?;
    Ok(Json(products))
}

/// GET /api/products/:id (public)
async fn get_product(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Product>, AppError> {
    let product = service::get_product(&state.pool, id).await?;
    Ok(Json(product))
}

/// POST /api/products (admin only)
async fn create_product(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(body): Json<CreateProductRequest>,
) -> Result<Json<Product>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let product = service::create_product(&state.pool, &body).await?;
    Ok(Json(product))
}

/// PUT /api/products/:id (admin only)
async fn update_product(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateProductRequest>,
) -> Result<Json<Product>, AppError> {
    let product = service::update_product(&state.pool, id, &body).await?;
    Ok(Json(product))
}

/// DELETE /api/products/:id (admin only — soft delete)
async fn delete_product(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    service::delete_product(&state.pool, id).await?;
    Ok(Json(serde_json::json!({"message": "Product deleted successfully"})))
}
