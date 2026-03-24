use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::AppError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: Uuid,    // user_id
    pub role: String, // USER or ADMIN
    pub exp: i64,     // expiry timestamp
    pub iat: i64,     // issued at
}

/// Create a JWT access token for the given user
pub fn create_access_token(
    user_id: Uuid,
    role: &str,
    secret: &str,
    expiry_minutes: i64,
) -> Result<String, AppError> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id,
        role: role.to_string(),
        exp: (now + Duration::minutes(expiry_minutes)).timestamp(),
        iat: now.timestamp(),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("Failed to create token: {}", e)))
}

/// Create a random refresh token string
pub fn create_refresh_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex::encode(&bytes)
}

/// Verify and decode a JWT access token
pub fn verify_access_token(token: &str, secret: &str) -> Result<Claims, AppError> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;

    Ok(token_data.claims)
}

/// Generate a 6-digit email verification code
pub fn generate_verification_code() -> String {
    let mut rng = rand::thread_rng();
    let code: u32 = rng.gen_range(100_000..999_999);
    code.to_string()
}
