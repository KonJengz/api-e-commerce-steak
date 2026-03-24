use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Product {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub image_public_id: Option<String>,
    pub current_price: Decimal,
    pub stock: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateProductRequest {
    #[validate(length(min = 1, message = "Product name is required"))]
    pub name: String,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub image_public_id: Option<String>,
    pub current_price: Decimal,
    pub stock: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProductRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub image_public_id: Option<String>,
    pub current_price: Option<Decimal>,
    pub stock: Option<i32>,
    pub is_active: Option<bool>,
}
