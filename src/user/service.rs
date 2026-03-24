use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::shared::errors::AppError;

use super::model::User;

/// Get user by ID
pub async fn get_user_by_id(pool: &PgPool, user_id: Uuid) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"SELECT id, email, role, is_active, is_verified, created_at, updated_at
           FROM users WHERE id = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    Ok(user)
}

/// Update user's email
pub async fn update_user_email(
    pool: &PgPool,
    user_id: Uuid,
    email: &str,
) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"UPDATE users SET email = $1, updated_at = $2
           WHERE id = $3
           RETURNING id, email, role, is_active, is_verified, created_at, updated_at"#,
    )
    .bind(email)
    .bind(Utc::now())
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(user)
}
