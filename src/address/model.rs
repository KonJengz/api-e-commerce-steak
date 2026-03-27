use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Address {
    pub id: Uuid,
    pub user_id: Uuid,
    pub recipient_name: String,
    pub phone: Option<String>,
    pub address_line: String,
    pub city: String,
    pub postal_code: String,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateAddressRequest {
    #[validate(length(min = 1, message = "Recipient name is required"))]
    pub recipient_name: String,
    pub phone: Option<String>,
    #[validate(length(min = 1, message = "Address line is required"))]
    pub address_line: String,
    #[validate(length(min = 1, message = "City is required"))]
    pub city: String,
    #[validate(length(min = 1, message = "Postal code is required"))]
    pub postal_code: String,
    pub is_default: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAddressRequest {
    pub recipient_name: Option<String>,
    pub phone: Option<String>,
    pub address_line: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
    pub is_default: Option<bool>,
}
