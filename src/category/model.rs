use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Category {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateCategoryRequest {
    #[validate(length(
        min = 1,
        max = 255,
        message = "Category name is required and must be at most 255 characters"
    ))]
    pub name: String,
    #[validate(length(max = 2000, message = "Description must be at most 2000 characters"))]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct UpdateCategoryRequest {
    #[validate(length(
        min = 1,
        max = 255,
        message = "Category name is required and must be at most 255 characters"
    ))]
    pub name: String,
    #[validate(length(max = 2000, message = "Description must be at most 2000 characters"))]
    pub description: Option<String>,
}
