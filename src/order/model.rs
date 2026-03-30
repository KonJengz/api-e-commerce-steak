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
    pub tracking_number: Option<String>,
    pub payment_slip_url: Option<String>,
    pub payment_submitted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OrderItem {
    pub id: Uuid,
    pub order_id: Uuid,
    pub product_id: Uuid,
    pub product_slug: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct AdminOrderListQuery {
    pub status: Option<String>,
    pub search: Option<String>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

impl AdminOrderListQuery {
    pub fn page(&self) -> i64 {
        self.page.unwrap_or(1).max(1)
    }

    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(20).clamp(1, 100)
    }

    pub fn offset(&self) -> i64 {
        (self.page() - 1) * self.limit()
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminOrder {
    pub id: Uuid,
    pub user_id: Uuid,
    pub user_name: String,
    pub user_email: String,
    pub shipping_address_id: Option<Uuid>,
    pub total_amount: Decimal,
    pub status: String,
    pub tracking_number: Option<String>,
    pub payment_slip_url: Option<String>,
    pub payment_submitted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ─── Response DTOs ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct OrderResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub shipping_address_id: Option<Uuid>,
    pub total_amount: Decimal,
    pub status: String,
    pub tracking_number: Option<String>,
    pub payment_slip_url: Option<String>,
    pub payment_submitted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Serialize)]
pub struct AdminOrderResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub user_name: String,
    pub user_email: String,
    pub shipping_address_id: Option<Uuid>,
    pub total_amount: Decimal,
    pub status: String,
    pub tracking_number: Option<String>,
    pub payment_slip_url: Option<String>,
    pub payment_submitted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Serialize)]
pub struct AdminOrderSummary {
    pub all: i64,
    pub pending: i64,
    pub payment_review: i64,
    pub payment_failed: i64,
    pub paid: i64,
    pub shipped: i64,
    pub delivered: i64,
    pub cancelled: i64,
    pub tracked: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminOrderListResponse {
    pub data: Vec<AdminOrder>,
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    pub total_pages: i64,
    pub summary: AdminOrderSummary,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct UpdateOrderRequest {
    #[validate(length(
        min = 1,
        max = 50,
        message = "Order status is required and must be at most 50 characters."
    ))]
    pub status: String,
    #[validate(length(max = 100, message = "Tracking number must be at most 100 characters."))]
    pub tracking_number: Option<String>,
}
