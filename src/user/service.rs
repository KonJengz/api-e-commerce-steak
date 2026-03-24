use chrono::{Duration, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::shared::errors::AppError;
use crate::shared::jwt;
use crate::shared::security::{
    EMAIL_VERIFICATION_PURPOSE_EMAIL_CHANGE, hash_verification_code, normalize_email,
};

use super::model::User;

const EMAIL_VERIFICATION_EXPIRY_MINUTES: i64 = 15;
const MAX_EMAIL_VERIFICATION_ATTEMPTS: i32 = 5;

#[derive(Debug, sqlx::FromRow)]
struct EmailChangeVerificationRecord {
    id: Uuid,
    email: String,
    code_hash: String,
    expires_at: chrono::DateTime<Utc>,
    attempt_count: i32,
}

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

/// Create an email-change verification record and return the plain verification code.
pub async fn request_email_change(
    pool: &PgPool,
    user_id: Uuid,
    new_email: &str,
    config: &AppConfig,
) -> Result<String, AppError> {
    let new_email = normalize_email(new_email)?;
    let current_user = get_user_by_id(pool, user_id).await?;

    if current_user.email == new_email {
        return Err(AppError::BadRequest(
            "New email must be different from the current email".to_string(),
        ));
    }

    let existing =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE email = $1 AND id != $2")
            .bind(&new_email)
            .bind(user_id)
            .fetch_one(pool)
            .await?;

    if existing > 0 {
        return Err(AppError::Conflict("Email already registered".to_string()));
    }

    let code = jwt::generate_verification_code();
    let code_hash = hash_verification_code(
        &config.jwt_secret,
        EMAIL_VERIFICATION_PURPOSE_EMAIL_CHANGE,
        &new_email,
        &code,
    );
    let expires_at = Utc::now() + Duration::minutes(EMAIL_VERIFICATION_EXPIRY_MINUTES);

    let mut tx = pool.begin().await?;
    replace_email_change_verification(&mut tx, user_id, &new_email, &code_hash, expires_at).await?;
    tx.commit().await?;

    Ok(code)
}

/// Verify a pending email change and update the user's profile.
pub async fn verify_email_change(
    pool: &PgPool,
    user_id: Uuid,
    email: &str,
    code: &str,
    config: &AppConfig,
) -> Result<User, AppError> {
    let email = normalize_email(email)?;
    let mut tx = pool.begin().await?;

    let record = match load_and_validate_email_change_verification(
        &mut tx,
        user_id,
        &email,
        code,
        &config.jwt_secret,
    )
    .await
    {
        Ok(record) => record,
        Err(err) => {
            tx.commit().await?;
            return Err(err);
        }
    };

    let existing =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE email = $1 AND id != $2")
            .bind(&record.email)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

    if existing > 0 {
        delete_email_change_verification(&mut tx, record.id).await?;
        tx.commit().await?;
        return Err(AppError::Conflict("Email already registered".to_string()));
    }

    let user = sqlx::query_as::<_, User>(
        r#"UPDATE users
           SET email = $1, is_verified = TRUE, updated_at = $2
           WHERE id = $3
           RETURNING id, email, role, is_active, is_verified, created_at, updated_at"#,
    )
    .bind(&record.email)
    .bind(Utc::now())
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    delete_email_change_verification(&mut tx, record.id).await?;
    tx.commit().await?;

    Ok(user)
}

async fn replace_email_change_verification(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    email: &str,
    code_hash: &str,
    expires_at: chrono::DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "DELETE FROM email_verifications WHERE purpose = $1 AND (user_id = $2 OR email = $3)",
    )
    .bind(EMAIL_VERIFICATION_PURPOSE_EMAIL_CHANGE)
    .bind(user_id)
    .bind(email)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"INSERT INTO email_verifications
           (id, email, code_hash, password_hash, user_id, purpose, expires_at, attempt_count)
           VALUES ($1, $2, $3, NULL, $4, $5, $6, 0)"#,
    )
    .bind(Uuid::now_v7())
    .bind(email)
    .bind(code_hash)
    .bind(user_id)
    .bind(EMAIL_VERIFICATION_PURPOSE_EMAIL_CHANGE)
    .bind(expires_at)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn load_and_validate_email_change_verification(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    email: &str,
    code: &str,
    secret: &str,
) -> Result<EmailChangeVerificationRecord, AppError> {
    let record = sqlx::query_as::<_, EmailChangeVerificationRecord>(
        r#"SELECT id, email, code_hash, expires_at, attempt_count
           FROM email_verifications
           WHERE email = $1 AND purpose = $2 AND user_id = $3
           ORDER BY created_at DESC
           LIMIT 1
           FOR UPDATE"#,
    )
    .bind(email)
    .bind(EMAIL_VERIFICATION_PURPOSE_EMAIL_CHANGE)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        AppError::BadRequest("No pending email change found for this email".to_string())
    })?;

    if Utc::now() > record.expires_at {
        delete_email_change_verification(tx, record.id).await?;
        return Err(AppError::BadRequest(
            "Verification code has expired".to_string(),
        ));
    }

    if record.attempt_count >= MAX_EMAIL_VERIFICATION_ATTEMPTS {
        delete_email_change_verification(tx, record.id).await?;
        return Err(AppError::TooManyRequests(
            "Too many verification attempts. Please request a new code.".to_string(),
        ));
    }

    let expected_hash =
        hash_verification_code(secret, EMAIL_VERIFICATION_PURPOSE_EMAIL_CHANGE, email, code);

    if expected_hash != record.code_hash {
        let next_attempt_count = record.attempt_count + 1;

        if next_attempt_count >= MAX_EMAIL_VERIFICATION_ATTEMPTS {
            delete_email_change_verification(tx, record.id).await?;
            return Err(AppError::TooManyRequests(
                "Too many verification attempts. Please request a new code.".to_string(),
            ));
        }

        sqlx::query("UPDATE email_verifications SET attempt_count = $1 WHERE id = $2")
            .bind(next_attempt_count)
            .bind(record.id)
            .execute(&mut **tx)
            .await?;

        return Err(AppError::BadRequest(
            "Invalid verification code".to_string(),
        ));
    }

    Ok(record)
}

async fn delete_email_change_verification(
    tx: &mut Transaction<'_, Postgres>,
    verification_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM email_verifications WHERE id = $1")
        .bind(verification_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}
