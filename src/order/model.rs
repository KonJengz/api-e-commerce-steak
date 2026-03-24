use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

// ─── Database Models ────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Order {
    pub id: Uuid,
    pub user_id: Uuid,
    pub shipping_address_id: Option<Uuid>,
    pub total_amount: Decimal,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OrderItem {
    pub id: Uuid,
    pub order_id: Uuid,
    pub product_id: Uuid,
    pub product_name_at_purchase: String,
    pub quantity: i32,
    pub price_at_purchase: Decimal,
}

// ─── Request DTOs ───────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateOrderRequest {
    pub shipping_address_id: Uuid,
    #[validate(length(min = 1, message = "At least one item is required"))]
    pub items: Vec<OrderItemRequest>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OrderItemRequest {
    pub product_id: Uuid,
    pub quantity: i32,
}

// ─── Response DTOs ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct OrderResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub shipping_address_id: Option<Uuid>,
    pub total_amount: Decimal,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub items: Vec<OrderItem>,
}
