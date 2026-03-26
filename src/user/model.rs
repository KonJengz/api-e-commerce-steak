use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

/// User database row
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub image: Option<String>,
    #[allow(dead_code)]
    #[serde(skip_serializing)]
    pub image_public_id: Option<String>,
    pub role: String,
    pub is_active: bool,
    pub is_verified: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// User profile response (excludes sensitive fields)
#[derive(Debug, Serialize)]
pub struct UserProfileResponse {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub image: Option<String>,
    pub role: String,
    pub is_active: bool,
    pub is_verified: bool,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserProfileResponse {
    fn from(u: User) -> Self {
        let User {
            id,
            name,
            email,
            image,
            image_public_id: _,
            role,
            is_active,
            is_verified,
            created_at,
            updated_at: _,
        } = u;

        Self {
            id,
            name,
            email,
            image,
            role,
            is_active,
            is_verified,
            created_at,
        }
    }
}

/// Update profile request
#[derive(Debug, Deserialize, Validate)]
pub struct RequestEmailChangeRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct VerifyEmailChangeRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
    #[validate(length(equal = 6, message = "Code must be 6 digits"))]
    pub code: String,
}

#[derive(Debug, Clone)]
pub enum ProfileImageUpdate<'a> {
    Keep,
    Remove,
    Replace {
        image_url: &'a str,
        image_public_id: &'a str,
    },
}
