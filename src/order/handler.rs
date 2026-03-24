use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use uuid::Uuid;
use validator::Validate;

use crate::shared::errors::AppError;
use crate::shared::extractors::AuthUser;
use crate::shared::pagination::{PaginatedResponse, PaginationQuery};
use crate::AppState;

use super::model::*;
use super::service;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_orders).post(create_order))
        .route("/{id}", get(get_order))
}

/// POST /api/orders
/// Create a new order — snapshots price from products at purchase time
async fn create_order(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateOrderRequest>,
) -> Result<Json<OrderResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let order = service::create_order(&state.pool, auth.user_id, &body).await?;
    Ok(Json(order))
}

/// GET /api/orders
async fn list_orders(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<PaginatedResponse<Order>>, AppError> {
    let orders = service::list_orders(&state.pool, auth.user_id, query).await?;
    Ok(Json(orders))
}

/// GET /api/orders/:id
async fn get_order(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<OrderResponse>, AppError> {
    let order = service::get_order(&state.pool, auth.user_id, id).await?;
    Ok(Json(order))
}
