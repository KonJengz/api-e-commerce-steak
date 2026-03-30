use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CartItem {
    pub id: Uuid,
    pub product_id: Uuid,
    pub product_slug: String,
    // Product snapshot for the frontend
    pub product_name: String,
    pub product_image_url: Option<String>,
    pub current_price: Decimal,
    pub stock: i32,
    pub is_active: bool,

    pub quantity: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct Cart {
    pub id: Uuid,
    pub user_id: Uuid,
    pub total_amount: Decimal,
    pub items: Vec<CartItem>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct AddCartItemRequest {
    pub product_id: Uuid,
    #[validate(range(min = 1, message = "Quantity must be at least 1"))]
    pub quantity: i32,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateCartItemRequest {
    #[validate(range(min = 0, message = "Quantity cannot be negative"))]
    pub quantity: i32,
}
