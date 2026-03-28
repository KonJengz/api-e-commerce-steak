use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use uuid::Uuid;
use validator::Validate;

use crate::AppState;
use crate::shared::background::spawn_app_task;
use crate::shared::email;
use crate::shared::errors::AppError;
use crate::shared::extractors::{AdminUser, AuthUser};
use crate::shared::pagination::{PaginatedResponse, PaginationQuery};

use super::model::*;
use super::service;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin", get(list_orders_for_admin))
        .route(
            "/admin/{id}",
            get(get_order_for_admin).put(update_order_for_admin),
        )
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

    let user_email = sqlx::query_scalar::<_, String>("SELECT email FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_one(&state.pool)
        .await?;

    let order = service::create_order(&state.pool, auth.user_id, &body).await?;

    // Send order confirmation email in background
    let config = state.config.clone();
    let order_id = order.id.to_string();
    let total = order.total_amount.to_string();

    spawn_app_task("send_order_confirmation", async move {
        email::send_order_confirmation(&user_email, &order_id, &total, &config).await
    });

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

/// GET /api/orders/admin
async fn list_orders_for_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(query): Query<AdminOrderListQuery>,
) -> Result<Json<PaginatedResponse<AdminOrder>>, AppError> {
    let orders = service::list_orders_for_admin(&state.pool, query).await?;
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

/// GET /api/orders/admin/:id
async fn get_order_for_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<AdminOrderResponse>, AppError> {
    let order = service::get_order_for_admin(&state.pool, id).await?;
    Ok(Json(order))
}

/// PUT /api/orders/admin/:id
async fn update_order_for_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateOrderRequest>,
) -> Result<Json<AdminOrderResponse>, AppError> {
    body.validate()
        .map_err(|error| AppError::BadRequest(error.to_string()))?;

    let (order, notification) = service::update_order_for_admin(&state.pool, id, &body).await?;

    if let Some(notification) = notification {
        let config = state.config.clone();

        spawn_app_task("send_order_tracking_email", async move {
            email::send_order_tracking_email(
                &notification.user_email,
                &notification.order_id,
                &notification.tracking_number,
                &config,
            )
            .await
        });
    }

    Ok(Json(order))
}
