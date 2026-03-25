use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use uuid::Uuid;
use validator::Validate;

use crate::AppState;
use crate::shared::errors::AppError;
use crate::shared::extractors::AuthUser;

use super::model::*;
use super::service;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_cart).delete(clear_cart))
        .route("/items", post(add_to_cart))
        .route(
            "/items/{product_id}",
            put(update_cart_item).delete(remove_from_cart),
        )
}

/// GET /api/carts
/// Gets the current user's cart
async fn get_cart(State(state): State<AppState>, user: AuthUser) -> Result<Json<Cart>, AppError> {
    let cart = service::get_or_create_cart(&state.pool, user.user_id).await?;
    let items = service::get_cart_items(&state.pool, cart.id).await?;

    // Calculate total amount dynamically
    let total_amount = items
        .iter()
        .map(|item| item.current_price * rust_decimal::Decimal::from(item.quantity))
        .sum();

    Ok(Json(Cart {
        id: cart.id,
        user_id: cart.user_id,
        items,
        total_amount,
        created_at: cart.created_at,
        updated_at: cart.updated_at,
    }))
}

/// POST /api/carts/items
/// Adds an item to the cart or increments its quantity
async fn add_to_cart(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<AddCartItemRequest>,
) -> Result<Json<Cart>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let cart = service::get_or_create_cart(&state.pool, user.user_id).await?;
    service::add_item_to_cart(&state.pool, cart.id, &body).await?;

    // Return the updated cart
    get_cart(State(state), user).await
}

/// PUT /api/carts/items/:product_id
/// Updates the exact quantity of an item
async fn update_cart_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(product_id): Path<Uuid>,
    Json(body): Json<UpdateCartItemRequest>,
) -> Result<Json<Cart>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let cart = service::get_or_create_cart(&state.pool, user.user_id).await?;

    if body.quantity == 0 {
        service::remove_item_from_cart(&state.pool, cart.id, product_id).await?;
    } else {
        service::update_cart_item_quantity(&state.pool, cart.id, product_id, body.quantity).await?;
    }

    get_cart(State(state), user).await
}

/// DELETE /api/carts/items/:product_id
/// Removes a specific product from the cart
async fn remove_from_cart(
    State(state): State<AppState>,
    user: AuthUser,
    Path(product_id): Path<Uuid>,
) -> Result<Json<Cart>, AppError> {
    let cart = service::get_or_create_cart(&state.pool, user.user_id).await?;
    service::remove_item_from_cart(&state.pool, cart.id, product_id).await?;

    get_cart(State(state), user).await
}

/// DELETE /api/carts
/// Clears all items from the cart
async fn clear_cart(State(state): State<AppState>, user: AuthUser) -> Result<Json<Cart>, AppError> {
    let cart = service::get_or_create_cart(&state.pool, user.user_id).await?;
    service::clear_cart(&state.pool, cart.id).await?;

    get_cart(State(state), user).await
}
