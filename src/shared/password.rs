use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use tokio::task;

use super::errors::AppError;

/// Hash a plain-text password with Argon2 (CPU-bound, runs in blocking thread)
pub async fn hash_password(password: String) -> Result<String, AppError> {
    task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| AppError::Internal(format!("Password hashing failed: {}", e)))?;

        Ok(hash.to_string())
    })
    .await
    .map_err(|e| AppError::Internal(format!("Task execution failed: {}", e)))?
}

/// Verify a plain-text password against a hash (CPU-bound, runs in blocking thread)
pub async fn verify_password(password: String, hash: String) -> Result<bool, AppError> {
    task::spawn_blocking(move || {
        let parsed_hash = PasswordHash::new(&hash)
            .map_err(|e| AppError::Internal(format!("Invalid password hash: {}", e)))?;

        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    })
    .await
    .map_err(|e| AppError::Internal(format!("Task execution failed: {}", e)))?
}
