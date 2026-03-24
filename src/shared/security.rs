use sha2::{Digest, Sha256};
use validator::ValidateEmail;

use super::errors::AppError;

pub const EMAIL_VERIFICATION_PURPOSE_REGISTER: &str = "REGISTER";
pub const EMAIL_VERIFICATION_PURPOSE_EMAIL_CHANGE: &str = "CHANGE_EMAIL";

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
    fn hash_refresh_token_is_deterministic() {
        let first = hash_refresh_token("secret", "token");
        let second = hash_refresh_token("secret", "token");
        assert_eq!(first, second);
    }
}
