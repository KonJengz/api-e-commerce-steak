use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::{Validate, ValidateEmail, ValidationError};

use crate::shared::security::USER_NAME_MAX_LEN;

// ─── Request DTOs ───────────────────────────────────────────

const LOGIN_EMAIL_MAX_LEN: usize = 255;
const LOGIN_PASSWORD_MIN_LEN: usize = 6;
const LOGIN_PASSWORD_MAX_LEN: usize = 128;

fn validation_error(message: &'static str, code: &'static str) -> ValidationError {
    let mut error = ValidationError::new(code);
    error.message = Some(Cow::Borrowed(message));
    error
}

fn validate_required_name(name: &str) -> Result<(), ValidationError> {
    let name = name.trim();
    let name_len = name.chars().count();

    if name.is_empty() {
        return Err(validation_error("Name is required.", "required"));
    }

    if name_len > USER_NAME_MAX_LEN {
        return Err(validation_error(
            "Name must be at most 100 characters.",
            "max_length",
        ));
    }

    Ok(())
}

fn validate_login_email(email: &str) -> Result<(), ValidationError> {
    let email = email.trim();

    if email.is_empty() {
        return Err(validation_error("Email is required.", "required"));
    }

    if email.len() > LOGIN_EMAIL_MAX_LEN {
        return Err(validation_error(
            "Email must be at most 255 characters.",
            "max_length",
        ));
    }

    if !email.validate_email() {
        return Err(validation_error(
            "Please enter a valid email address.",
            "email",
        ));
    }

    Ok(())
}

fn validate_login_password(password: &str) -> Result<(), ValidationError> {
    let password = password.trim();
    let password_len = password.chars().count();

    if password.is_empty() {
        return Err(validation_error("Password is required.", "required"));
    }

    if password_len < LOGIN_PASSWORD_MIN_LEN {
        return Err(validation_error(
            "Password must be at least 6 characters.",
            "min_length",
        ));
    }

    if password_len > LOGIN_PASSWORD_MAX_LEN {
        return Err(validation_error(
            "Password must be at most 128 characters.",
            "max_length",
        ));
    }

    Ok(())
}

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(custom(function = "validate_required_name"))]
    pub name: String,
    #[validate(
        email(message = "Invalid email address"),
        length(max = 255, message = "Email must be at most 255 characters.")
    )]
    pub email: String,
    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub password: String,
    pub image: Option<String>,
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
    #[validate(custom(function = "validate_login_email"))]
    pub email: String,
    #[validate(custom(function = "validate_login_password"))]
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

#[derive(Debug, Deserialize, Validate)]
pub struct OauthExchangeRequest {
    #[validate(length(min = 1, message = "Ticket cannot be empty"))]
    pub ticket: String,
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
    pub name: String,
    pub email: String,
    pub image: Option<String>,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_request_rejects_blank_email_after_trim() {
        let request = LoginRequest {
            email: "   ".to_string(),
            password: "secret123".to_string(),
        };

        let error = request.validate().expect_err("validation should fail");
        assert!(error.to_string().contains("Email is required."));
    }

    #[test]
    fn login_request_rejects_email_longer_than_255_chars() {
        let request = LoginRequest {
            email: format!("{}@example.com", "a".repeat(245)),
            password: "secret123".to_string(),
        };

        let error = request.validate().expect_err("validation should fail");
        assert!(
            error
                .to_string()
                .contains("Email must be at most 255 characters.")
        );
    }

    #[test]
    fn login_request_rejects_short_password_after_trim() {
        let request = LoginRequest {
            email: "user@example.com".to_string(),
            password: "  12345  ".to_string(),
        };

        let error = request.validate().expect_err("validation should fail");
        assert!(
            error
                .to_string()
                .contains("Password must be at least 6 characters.")
        );
    }

    #[test]
    fn login_request_accepts_values_with_surrounding_whitespace() {
        let request = LoginRequest {
            email: "  user@example.com  ".to_string(),
            password: "  secret123  ".to_string(),
        };

        request.validate().expect("validation should pass");
    }

    #[test]
    fn register_request_rejects_blank_name_after_trim() {
        let request = RegisterRequest {
            name: "   ".to_string(),
            email: "user@example.com".to_string(),
            password: "12345678".to_string(),
            image: None,
        };

        let error = request.validate().expect_err("validation should fail");
        assert!(error.to_string().contains("Name is required."));
    }

    #[test]
    fn register_request_accepts_optional_image() {
        let request = RegisterRequest {
            name: "Jane Doe".to_string(),
            email: "user@example.com".to_string(),
            password: "12345678".to_string(),
            image: Some("https://cdn.example.com/avatar.png".to_string()),
        };

        request.validate().expect("validation should pass");
    }
}
