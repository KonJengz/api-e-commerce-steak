use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

/// User database row
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
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
    pub email: String,
    pub role: String,
    pub is_active: bool,
    pub is_verified: bool,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserProfileResponse {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            email: u.email,
            role: u.role,
            is_active: u.is_active,
            is_verified: u.is_verified,
            created_at: u.created_at,
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
