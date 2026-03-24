use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use uuid::Uuid;
use validator::Validate;

use crate::shared::errors::AppError;
use crate::shared::extractors::AuthUser;
use crate::AppState;

use super::model::*;
use super::service;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_addresses).post(create_address))
        .route("/{id}", get(get_address).put(update_address).delete(delete_address))
}

/// GET /api/addresses
async fn list_addresses(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Address>>, AppError> {
    let addresses = service::list_addresses(&state.pool, auth.user_id).await?;
    Ok(Json(addresses))
}

/// GET /api/addresses/:id
async fn get_address(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Address>, AppError> {
    let addresses = service::list_addresses(&state.pool, auth.user_id).await?;
    let address = addresses
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::NotFound("Address not found".to_string()))?;
    Ok(Json(address))
}

/// POST /api/addresses
async fn create_address(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateAddressRequest>,
) -> Result<Json<Address>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let address = service::create_address(&state.pool, auth.user_id, &body).await?;
    Ok(Json(address))
}

/// PUT /api/addresses/:id
async fn update_address(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAddressRequest>,
) -> Result<Json<Address>, AppError> {
    let address = service::update_address(&state.pool, auth.user_id, id, &body).await?;
    Ok(Json(address))
}

/// DELETE /api/addresses/:id
async fn delete_address(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    service::delete_address(&state.pool, auth.user_id, id).await?;
    Ok(Json(serde_json::json!({"message": "Address deleted successfully"})))
}
