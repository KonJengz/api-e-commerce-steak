use sha2::{Digest, Sha256};
use validator::ValidateEmail;

use super::errors::AppError;

pub const EMAIL_VERIFICATION_PURPOSE_REGISTER: &str = "REGISTER";
pub const EMAIL_VERIFICATION_PURPOSE_EMAIL_CHANGE: &str = "CHANGE_EMAIL";
pub const USER_NAME_MAX_LEN: usize = 100;

fn hash_segments(secret: &str, segments: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());

    for segment in segments {
        hasher.update([0]);
        hasher.update(segment.as_bytes());
    }

    hex::encode(hasher.finalize())
}

pub fn normalize_email(email: &str) -> Result<String, AppError> {
    let normalized = email.trim().to_ascii_lowercase();

    if normalized.is_empty() || !normalized.validate_email() {
        return Err(AppError::BadRequest("Invalid email address".to_string()));
    }

    Ok(normalized)
}

pub fn normalize_required_name(name: &str) -> Result<String, AppError> {
    let normalized = name.trim();
    let name_len = normalized.chars().count();

    if normalized.is_empty() {
        return Err(AppError::BadRequest("Name is required.".to_string()));
    }

    if name_len > USER_NAME_MAX_LEN {
        return Err(AppError::BadRequest(format!(
            "Name must be at most {} characters.",
            USER_NAME_MAX_LEN
        )));
    }

    Ok(normalized.to_string())
}

pub fn normalize_optional_image(image: Option<&str>) -> Option<String> {
    image
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

pub fn fallback_user_name(email: &str) -> String {
    let local_part = email
        .split('@')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("User");

    local_part.chars().take(USER_NAME_MAX_LEN).collect()
}

pub fn coerce_oauth_user_name(name: Option<&str>, email: &str) -> String {
    match name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(USER_NAME_MAX_LEN).collect::<String>())
    {
        Some(value) if !value.is_empty() => value,
        _ => fallback_user_name(email),
    }
}

pub fn hash_refresh_token(secret: &str, token: &str) -> String {
    hash_segments(secret, &["refresh_token", token])
}

pub fn hash_verification_code(secret: &str, purpose: &str, email: &str, code: &str) -> String {
    hash_segments(secret, &["email_verification", purpose, email, code])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_email_trims_and_lowercases() {
        let normalized = normalize_email("  User@Example.COM ").expect("email should normalize");
        assert_eq!(normalized, "user@example.com");
    }

    #[test]
    fn normalize_required_name_trims_input() {
        let normalized = normalize_required_name("  Jane Doe  ").expect("name should normalize");
        assert_eq!(normalized, "Jane Doe");
    }

    #[test]
    fn fallback_user_name_uses_email_local_part() {
        assert_eq!(fallback_user_name(" shopper@example.com "), "shopper");
    }

    #[test]
    fn normalize_optional_image_drops_blank_strings() {
        assert_eq!(normalize_optional_image(Some("   ")), None);
    }

    #[test]
    fn hash_refresh_token_is_deterministic() {
        let first = hash_refresh_token("secret", "token");
        let second = hash_refresh_token("secret", "token");
        assert_eq!(first, second);
    }
}
