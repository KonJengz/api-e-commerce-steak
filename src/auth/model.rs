use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

// ─── Request DTOs ───────────────────────────────────────────

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct VerifyEmailRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
    #[validate(length(equal = 6, message = "Code must be 6 digits"))]
    pub code: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct GoogleLoginRequest {
    #[validate(length(min = 1, message = "Token cannot be empty"))]
    pub token: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct GithubLoginRequest {
    #[validate(length(min = 1, message = "Code cannot be empty"))]
    pub code: String,
}

// ─── Response DTOs ──────────────────────────────────────────

/// Auth response — refresh_token is sent as HttpOnly cookie, not in body
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub user: UserInfo,
}

/// Internal struct used by service layer (includes refresh_token for cookie setting)
#[derive(Debug)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserInfo,
}

#[derive(Debug, Serialize, Clone)]
pub struct UserInfo {
    pub id: Uuid,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message: String,
}
