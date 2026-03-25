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

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ProductImage {
    pub id: Uuid,
    pub product_id: Uuid,
    pub image_url: String,
    pub image_public_id: String,
    pub sort_order: i32,
    pub is_primary: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ProductImageMutationResponse {
    pub product: Product,
    pub images: Vec<ProductImage>,
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

#[derive(Debug, Deserialize, Validate)]
pub struct AttachProductImageRequest {
    #[validate(length(min = 1, message = "image_url is required"))]
    pub image_url: String,
    #[validate(length(min = 1, message = "image_public_id is required"))]
    pub image_public_id: String,
    pub is_primary: Option<bool>,
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

#[derive(Debug, Deserialize, Validate)]
pub struct ReorderProductImagesRequest {
    #[validate(length(min = 1, message = "At least one image id is required"))]
    pub image_ids: Vec<Uuid>,
}
