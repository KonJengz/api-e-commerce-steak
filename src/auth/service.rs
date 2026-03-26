use chrono::{Duration, Utc};
use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::shared::errors::AppError;
use crate::shared::jwt;
use crate::shared::password;
use crate::shared::security::{
    EMAIL_VERIFICATION_PURPOSE_PASSWORD_RESET, EMAIL_VERIFICATION_PURPOSE_REGISTER,
    coerce_oauth_user_name, fallback_user_name, hash_oauth_login_ticket, hash_refresh_token,
    hash_verification_code, normalize_email, normalize_optional_image, normalize_required_name,
};

use super::model::{AuthTokens, UserInfo};

const EMAIL_VERIFICATION_EXPIRY_MINUTES: i64 = 15;
const MAX_EMAIL_VERIFICATION_ATTEMPTS: i32 = 5;
const OAUTH_LOGIN_TICKET_EXPIRY_MINUTES: i64 = 5;
const GOOGLE_PROVIDER_NAME: &str = "google";
const GITHUB_PROVIDER_NAME: &str = "github";
const SOCIAL_ACCOUNT_EXISTS_MESSAGE: &str = "An account with this email already exists. Please sign in with your existing method to continue.";

#[derive(Debug, Deserialize)]
struct GoogleTokenInfo {
    aud: String,
    email: String,
    email_verified: serde_json::Value,
    name: Option<String>,
    picture: Option<String>,
    sub: String,
    nonce: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct GoogleAuthorizationCodeResponse {
    id_token: String,
}

#[derive(Debug, Deserialize)]
struct GithubUser {
    id: i64,
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubEmail {
    email: String,
    primary: bool,
    verified: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct EmailVerificationRecord {
    id: Uuid,
    email: String,
    name: Option<String>,
    image: Option<String>,
    code_hash: String,
    password_hash: Option<String>,
    expires_at: chrono::DateTime<Utc>,
    attempt_count: i32,
}

struct VerificationSnapshot<'a> {
    name: Option<&'a str>,
    image: Option<&'a str>,
    password_hash: Option<&'a str>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct AuthUserRecord {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) email: String,
    pub(crate) image: Option<String>,
    pub(crate) role: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct SocialUserRecord {
    id: Uuid,
    name: String,
    email: String,
    image: Option<String>,
    role: String,
    is_active: bool,
    is_verified: bool,
}

struct SocialIdentity {
    provider_name: &'static str,
    provider_label: &'static str,
    provider_id: String,
    email: String,
    name: String,
    image: Option<String>,
}

impl From<AuthUserRecord> for UserInfo {
    fn from(user: AuthUserRecord) -> Self {
        Self {
            id: user.id,
            name: user.name,
            email: user.email,
            image: user.image,
            role: user.role,
        }
    }
}

impl From<SocialUserRecord> for AuthUserRecord {
    fn from(user: SocialUserRecord) -> Self {
        Self {
            id: user.id,
            name: user.name,
            email: user.email,
            image: user.image,
            role: user.role,
        }
    }
}

/// Create email verification record and return verification code
pub async fn create_email_verification(
    pool: &PgPool,
    name: &str,
    email: &str,
    plain_password: &str,
    image: Option<&str>,
    config: &AppConfig,
) -> Result<String, AppError> {
    let name = normalize_required_name(name)?;
    let email = normalize_email(email)?;
    let image = normalize_optional_image(image);
    let hashed_password = password::hash_password(plain_password.trim().to_string()).await?;
    let now = Utc::now();

    let mut tx = pool.begin().await?;
    let existing_user = sqlx::query_as::<_, (Uuid, bool, bool)>(
        r#"SELECT id, is_active, is_verified
           FROM users
           WHERE email = $1
           FOR UPDATE"#,
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;

    let user_id = match existing_user {
        Some((existing_user_id, is_active, is_verified)) => {
            if !is_active {
                return Err(AppError::Forbidden("Account is suspended".to_string()));
            }

            if is_verified {
                return Err(AppError::Conflict("Email already registered".to_string()));
            }

            sqlx::query(
                r#"UPDATE users
                   SET name = $1, image = $2, password_hash = $3, updated_at = $4
                   WHERE id = $5"#,
            )
            .bind(&name)
            .bind(image.as_deref())
            .bind(&hashed_password)
            .bind(now)
            .bind(existing_user_id)
            .execute(&mut *tx)
            .await?;

            existing_user_id
        }
        None => {
            let new_user_id = Uuid::now_v7();

            sqlx::query(
                r#"INSERT INTO users (id, name, email, image, password_hash, role, is_active, is_verified, created_at, updated_at)
                   VALUES ($1, $2, $3, $4, $5, 'USER', TRUE, FALSE, $6, $6)"#,
            )
            .bind(new_user_id)
            .bind(&name)
            .bind(&email)
            .bind(image.as_deref())
            .bind(&hashed_password)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            new_user_id
        }
    };

    let code = jwt::generate_verification_code();
    let code_hash = hash_verification_code(
        &config.jwt_secret,
        EMAIL_VERIFICATION_PURPOSE_REGISTER,
        &email,
        &code,
    );
    let expires_at = Utc::now() + Duration::minutes(EMAIL_VERIFICATION_EXPIRY_MINUTES);

    replace_email_verification(
        &mut tx,
        &email,
        EMAIL_VERIFICATION_PURPOSE_REGISTER,
        Some(user_id),
        VerificationSnapshot {
            name: None,
            image: None,
            password_hash: None,
        },
        &code_hash,
        expires_at,
    )
    .await?;
    tx.commit().await?;

    Ok(code)
}

/// Delete any pending registration verification for the given email.
pub async fn clear_pending_email_verification(pool: &PgPool, email: &str) -> Result<(), AppError> {
    let email = normalize_email(email)?;

    sqlx::query("DELETE FROM email_verifications WHERE purpose = $1 AND email = $2")
        .bind(EMAIL_VERIFICATION_PURPOSE_REGISTER)
        .bind(email)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn resend_email_verification(
    pool: &PgPool,
    email: &str,
    config: &AppConfig,
) -> Result<String, AppError> {
    let email = normalize_email(email)?;
    let now = Utc::now();
    let mut tx = pool.begin().await?;
    let existing_user = sqlx::query_as::<_, (Uuid, bool, bool)>(
        r#"SELECT id, is_active, is_verified
           FROM users
           WHERE email = $1
           FOR UPDATE"#,
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;

    let (user_id, legacy_name, legacy_image, legacy_password_hash) = match existing_user {
        Some((user_id, is_active, is_verified)) => {
            if !is_active {
                return Err(AppError::Forbidden("Account is suspended".to_string()));
            }

            if is_verified {
                return Err(AppError::BadRequest("Email already verified".to_string()));
            }

            sqlx::query("UPDATE users SET updated_at = $1 WHERE id = $2")
                .bind(now)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;

            (Some(user_id), None, None, None)
        }
        None => {
            let legacy_record = sqlx::query_as::<_, EmailVerificationRecord>(
                r#"SELECT id, email, name, image, code_hash, password_hash, expires_at, attempt_count
                   FROM email_verifications
                   WHERE email = $1 AND purpose = $2 AND user_id IS NULL
                   ORDER BY created_at DESC
                   LIMIT 1
                   FOR UPDATE"#,
            )
            .bind(&email)
            .bind(EMAIL_VERIFICATION_PURPOSE_REGISTER)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                AppError::BadRequest("No pending verification found for this email".to_string())
            })?;

            if legacy_record.password_hash.is_none() {
                return Err(AppError::Internal(
                    "Missing password hash for legacy pending registration".to_string(),
                ));
            }

            (
                None,
                legacy_record.name,
                legacy_record.image,
                legacy_record.password_hash,
            )
        }
    };

    let code = jwt::generate_verification_code();
    let code_hash = hash_verification_code(
        &config.jwt_secret,
        EMAIL_VERIFICATION_PURPOSE_REGISTER,
        &email,
        &code,
    );
    let expires_at = Utc::now() + Duration::minutes(EMAIL_VERIFICATION_EXPIRY_MINUTES);

    replace_email_verification(
        &mut tx,
        &email,
        EMAIL_VERIFICATION_PURPOSE_REGISTER,
        user_id,
        VerificationSnapshot {
            name: legacy_name.as_deref(),
            image: legacy_image.as_deref(),
            password_hash: legacy_password_hash.as_deref(),
        },
        &code_hash,
        expires_at,
    )
    .await?;
    tx.commit().await?;

    Ok(code)
}

pub async fn request_password_reset(
    pool: &PgPool,
    email: &str,
    config: &AppConfig,
) -> Result<Option<String>, AppError> {
    let email = normalize_email(email)?;

    let user = sqlx::query_as::<_, (bool, bool)>(
        r#"SELECT is_active, is_verified
           FROM users
           WHERE email = $1"#,
    )
    .bind(&email)
    .fetch_optional(pool)
    .await?;

    let Some((is_active, is_verified)) = user else {
        return Ok(None);
    };

    if !is_active || !is_verified {
        return Ok(None);
    }

    let code = jwt::generate_verification_code();
    let code_hash = hash_verification_code(
        &config.jwt_secret,
        EMAIL_VERIFICATION_PURPOSE_PASSWORD_RESET,
        &email,
        &code,
    );
    let expires_at = Utc::now() + Duration::minutes(EMAIL_VERIFICATION_EXPIRY_MINUTES);

    let mut tx = pool.begin().await?;
    replace_email_verification(
        &mut tx,
        &email,
        EMAIL_VERIFICATION_PURPOSE_PASSWORD_RESET,
        None,
        VerificationSnapshot {
            name: None,
            image: None,
            password_hash: None,
        },
        &code_hash,
        expires_at,
    )
    .await?;
    tx.commit().await?;

    Ok(Some(code))
}

pub async fn clear_pending_password_reset(pool: &PgPool, email: &str) -> Result<(), AppError> {
    let email = normalize_email(email)?;

    sqlx::query("DELETE FROM email_verifications WHERE purpose = $1 AND email = $2")
        .bind(EMAIL_VERIFICATION_PURPOSE_PASSWORD_RESET)
        .bind(email)
        .execute(pool)
        .await?;

    Ok(())
}

/// Verify email code, activate the user, and issue tokens.
pub async fn verify_email_and_issue_tokens(
    pool: &PgPool,
    email: &str,
    code: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let email = normalize_email(email)?;
    let mut tx = pool.begin().await?;

    let user = sqlx::query_as::<_, (Uuid, String, String, Option<String>, String, bool, bool)>(
        r#"SELECT id, name, email, image, role, is_active, is_verified
           FROM users
           WHERE email = $1
           FOR UPDATE"#,
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;

    match user {
        Some((user_id, user_name, user_email, user_image, role, is_active, is_verified)) => {
            if !is_active {
                tx.commit().await?;
                return Err(AppError::Forbidden("Account is suspended".to_string()));
            }

            if is_verified {
                tx.commit().await?;
                return Err(AppError::BadRequest("Email already verified".to_string()));
            }

            let record = match load_and_validate_email_verification(
                &mut tx,
                &email,
                EMAIL_VERIFICATION_PURPOSE_REGISTER,
                Some(user_id),
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

            let now = Utc::now();

            sqlx::query(
                r#"UPDATE users
                   SET is_verified = TRUE, updated_at = $1
                   WHERE id = $2"#,
            )
            .bind(now)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

            delete_email_verification(&mut tx, record.id).await?;
            tx.commit().await?;

            let access_token = jwt::create_access_token(
                user_id,
                &role,
                &config.jwt_secret,
                config.jwt_access_expiry_minutes,
            )?;
            let refresh_token = create_refresh_token(pool, user_id, config).await?;

            Ok(AuthTokens {
                access_token,
                refresh_token,
                user: UserInfo {
                    id: user_id,
                    name: user_name,
                    email: user_email,
                    image: user_image,
                    role,
                },
            })
        }
        None => {
            let record = match load_and_validate_email_verification(
                &mut tx,
                &email,
                EMAIL_VERIFICATION_PURPOSE_REGISTER,
                None,
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

            let password_hash = record.password_hash.as_deref().ok_or_else(|| {
                AppError::Internal(
                    "Missing password hash for legacy pending registration".to_string(),
                )
            })?;
            let user_id = Uuid::now_v7();
            let name = record
                .name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
                .unwrap_or_else(|| fallback_user_name(&record.email));
            let image = normalize_optional_image(record.image.as_deref());
            let now = Utc::now();
            let role = "USER".to_string();

            sqlx::query(
                r#"INSERT INTO users (id, name, email, image, password_hash, role, is_active, is_verified, created_at, updated_at)
                   VALUES ($1, $2, $3, $4, $5, $6, TRUE, TRUE, $7, $7)"#,
            )
            .bind(user_id)
            .bind(&name)
            .bind(&record.email)
            .bind(image.as_deref())
            .bind(password_hash)
            .bind(&role)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            delete_email_verification(&mut tx, record.id).await?;
            tx.commit().await?;

            let access_token = jwt::create_access_token(
                user_id,
                &role,
                &config.jwt_secret,
                config.jwt_access_expiry_minutes,
            )?;
            let refresh_token = create_refresh_token(pool, user_id, config).await?;

            Ok(AuthTokens {
                access_token,
                refresh_token,
                user: UserInfo {
                    id: user_id,
                    name,
                    email: record.email,
                    image,
                    role,
                },
            })
        }
    }
}

/// Authenticate user with email/password and return tokens
pub async fn login(
    pool: &PgPool,
    email: &str,
    plain_password: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let email = normalize_email(email)?;

    let user = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            bool,
            bool,
        ),
    >(
        r#"SELECT id, name, email, image, password_hash, role, is_active, is_verified
           FROM users WHERE email = $1"#,
    )
    .bind(&email)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid email or password".to_string()))?;

    let (user_id, user_name, user_email, user_image, password_hash, role, is_active, is_verified) =
        user;

    if !is_active {
        return Err(AppError::Forbidden("Account is suspended".to_string()));
    }

    let password_hash = password_hash.ok_or_else(|| {
        AppError::Unauthorized("Please login with your social account".to_string())
    })?;

    let is_valid = password::verify_password(plain_password.to_string(), password_hash).await?;
    if !is_valid {
        return Err(AppError::Unauthorized(
            "Invalid email or password".to_string(),
        ));
    }

    if !is_verified {
        return Err(AppError::Forbidden("Email not verified".to_string()));
    }

    let access_token = jwt::create_access_token(
        user_id,
        &role,
        &config.jwt_secret,
        config.jwt_access_expiry_minutes,
    )?;
    let refresh_token = create_refresh_token(pool, user_id, config).await?;

    Ok(AuthTokens {
        access_token,
        refresh_token,
        user: UserInfo {
            id: user_id,
            name: user_name,
            email: user_email,
            image: user_image,
            role,
        },
    })
}

pub async fn reset_password(
    pool: &PgPool,
    email: &str,
    code: &str,
    new_password: &str,
    config: &AppConfig,
) -> Result<(), AppError> {
    let email = normalize_email(email)?;
    let new_password = new_password.trim();
    let mut tx = pool.begin().await?;

    let record = match load_and_validate_email_verification(
        &mut tx,
        &email,
        EMAIL_VERIFICATION_PURPOSE_PASSWORD_RESET,
        None,
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

    let user = sqlx::query_as::<_, (Uuid, Option<String>, bool, bool)>(
        r#"SELECT id, password_hash, is_active, is_verified
           FROM users
           WHERE email = $1
           FOR UPDATE"#,
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::BadRequest("Invalid password reset request".to_string()))?;

    let (user_id, password_hash, is_active, is_verified) = user;

    if !is_active {
        tx.commit().await?;
        return Err(AppError::Forbidden("Account is suspended".to_string()));
    }

    if !is_verified {
        tx.commit().await?;
        return Err(AppError::BadRequest(
            "Email verification is required before resetting password".to_string(),
        ));
    }

    ensure_password_is_new(password_hash.as_deref(), new_password).await?;
    let next_password_hash = password::hash_password(new_password.to_string()).await?;

    sqlx::query(
        r#"UPDATE users
           SET password_hash = $1, updated_at = $2
           WHERE id = $3"#,
    )
    .bind(&next_password_hash)
    .bind(Utc::now())
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    delete_email_verification(&mut tx, record.id).await?;
    delete_refresh_tokens_for_user_in_tx(&mut tx, user_id).await?;
    tx.commit().await?;

    Ok(())
}

pub async fn change_password(
    pool: &PgPool,
    user_id: Uuid,
    current_password: &str,
    new_password: &str,
) -> Result<(), AppError> {
    let current_password = current_password.trim();
    let new_password = new_password.trim();
    let mut tx = pool.begin().await?;

    let user = sqlx::query_as::<_, (Option<String>, bool)>(
        r#"SELECT password_hash, is_active
           FROM users
           WHERE id = $1
           FOR UPDATE"#,
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::Unauthorized("User no longer exists".to_string()))?;

    let (password_hash, is_active) = user;

    if !is_active {
        tx.commit().await?;
        return Err(AppError::Forbidden("Account is suspended".to_string()));
    }

    let password_hash = match password_hash {
        Some(value) => value,
        None => {
            tx.commit().await?;
            return Err(AppError::BadRequest(
                "Password is not set for this account. Use set password instead.".to_string(),
            ));
        }
    };

    let is_valid =
        password::verify_password(current_password.to_string(), password_hash.clone()).await?;
    if !is_valid {
        tx.commit().await?;
        return Err(AppError::Unauthorized(
            "Current password is incorrect".to_string(),
        ));
    }

    ensure_password_is_new(Some(password_hash.as_str()), new_password).await?;
    let next_password_hash = password::hash_password(new_password.to_string()).await?;

    sqlx::query(
        r#"UPDATE users
           SET password_hash = $1, updated_at = $2
           WHERE id = $3"#,
    )
    .bind(&next_password_hash)
    .bind(Utc::now())
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    delete_refresh_tokens_for_user_in_tx(&mut tx, user_id).await?;
    tx.commit().await?;

    Ok(())
}

pub async fn set_password(
    pool: &PgPool,
    user_id: Uuid,
    new_password: &str,
) -> Result<(), AppError> {
    let new_password = new_password.trim();
    let mut tx = pool.begin().await?;

    let user = sqlx::query_as::<_, (Option<String>, bool)>(
        r#"SELECT password_hash, is_active
           FROM users
           WHERE id = $1
           FOR UPDATE"#,
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::Unauthorized("User no longer exists".to_string()))?;

    let (password_hash, is_active) = user;

    if !is_active {
        tx.commit().await?;
        return Err(AppError::Forbidden("Account is suspended".to_string()));
    }

    if password_hash.is_some() {
        tx.commit().await?;
        return Err(AppError::BadRequest(
            "Password is already set for this account. Use change password instead.".to_string(),
        ));
    }

    let next_password_hash = password::hash_password(new_password.to_string()).await?;

    sqlx::query(
        r#"UPDATE users
           SET password_hash = $1, updated_at = $2
           WHERE id = $3"#,
    )
    .bind(&next_password_hash)
    .bind(Utc::now())
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    delete_refresh_tokens_for_user_in_tx(&mut tx, user_id).await?;
    tx.commit().await?;

    Ok(())
}

async fn load_social_user_by_provider(
    tx: &mut Transaction<'_, Postgres>,
    provider_name: &str,
    provider_id: &str,
) -> Result<Option<SocialUserRecord>, AppError> {
    sqlx::query_as::<_, SocialUserRecord>(
        r#"SELECT u.id, u.name, u.email, u.image, u.role, u.is_active, u.is_verified
           FROM account_providers ap
           JOIN users u ON u.id = ap.user_id
           WHERE ap.provider_name = $1 AND ap.provider_id = $2"#,
    )
    .bind(provider_name)
    .bind(provider_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn load_social_user_by_email(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
) -> Result<Option<SocialUserRecord>, AppError> {
    sqlx::query_as::<_, SocialUserRecord>(
        r#"SELECT id, name, email, image, role, is_active, is_verified
           FROM users
           WHERE email = $1
           FOR UPDATE"#,
    )
    .bind(email)
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn load_social_user_by_id(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Option<SocialUserRecord>, AppError> {
    sqlx::query_as::<_, SocialUserRecord>(
        r#"SELECT id, name, email, image, role, is_active, is_verified
           FROM users
           WHERE id = $1
           FOR UPDATE"#,
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn load_existing_provider_link(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    provider_name: &str,
) -> Result<Option<String>, AppError> {
    sqlx::query_scalar::<_, String>(
        r#"SELECT provider_id
           FROM account_providers
           WHERE user_id = $1 AND provider_name = $2"#,
    )
    .bind(user_id)
    .bind(provider_name)
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn insert_provider_link(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    provider_name: &'static str,
    provider_label: &'static str,
    provider_id: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO account_providers (id, user_id, provider_name, provider_id)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(provider_name)
    .bind(provider_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| match error {
        sqlx::Error::Database(db_error) if db_error.code().as_deref() == Some("23505") => {
            AppError::Conflict(format!(
                "This {} account is already linked to another user.",
                provider_label
            ))
        }
        other => other.into(),
    })?;

    Ok(())
}

async fn create_social_user(
    tx: &mut Transaction<'_, Postgres>,
    identity: &SocialIdentity,
) -> Result<AuthUserRecord, AppError> {
    let now = Utc::now();
    let user = sqlx::query_as::<_, AuthUserRecord>(
        r#"INSERT INTO users (id, name, email, image, password_hash, role, is_active, is_verified, created_at, updated_at)
           VALUES ($1, $2, $3, $4, NULL, 'USER', TRUE, TRUE, $5, $5)
           RETURNING id, name, email, image, role"#,
    )
    .bind(Uuid::now_v7())
    .bind(&identity.name)
    .bind(&identity.email)
    .bind(identity.image.as_deref())
    .bind(now)
    .fetch_one(&mut **tx)
    .await?;

    insert_provider_link(
        tx,
        user.id,
        identity.provider_name,
        identity.provider_label,
        &identity.provider_id,
    )
    .await?;

    Ok(user)
}

async fn maybe_mark_user_verified_from_social(
    tx: &mut Transaction<'_, Postgres>,
    user: &SocialUserRecord,
    social_email: &str,
) -> Result<(), AppError> {
    if user.is_verified || user.email != social_email {
        return Ok(());
    }

    sqlx::query("UPDATE users SET is_verified = TRUE, updated_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(user.id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

async fn resolve_social_user(
    tx: &mut Transaction<'_, Postgres>,
    identity: &SocialIdentity,
) -> Result<AuthUserRecord, AppError> {
    if let Some(user) =
        load_social_user_by_provider(tx, identity.provider_name, &identity.provider_id).await?
    {
        if !user.is_active {
            return Err(AppError::Forbidden("Account is suspended".to_string()));
        }

        maybe_mark_user_verified_from_social(tx, &user, &identity.email).await?;

        return Ok(user.into());
    }

    let existing_user = load_social_user_by_email(tx, &identity.email).await?;

    match existing_user {
        Some(user) => {
            if !user.is_active {
                return Err(AppError::Forbidden("Account is suspended".to_string()));
            }

            if let Some(existing_provider_id) =
                load_existing_provider_link(tx, user.id, identity.provider_name).await?
            {
                if existing_provider_id != identity.provider_id.as_str() {
                    return Err(AppError::Conflict(format!(
                        "This account is already linked to a different {} account.",
                        identity.provider_label
                    )));
                }

                maybe_mark_user_verified_from_social(tx, &user, &identity.email).await?;

                return Ok(user.into());
            }

            if user.is_verified {
                return Err(AppError::Conflict(
                    SOCIAL_ACCOUNT_EXISTS_MESSAGE.to_string(),
                ));
            }

            insert_provider_link(
                tx,
                user.id,
                identity.provider_name,
                identity.provider_label,
                &identity.provider_id,
            )
            .await?;

            maybe_mark_user_verified_from_social(tx, &user, &identity.email).await?;

            Ok(user.into())
        }
        None => create_social_user(tx, identity).await,
    }
}

async fn link_social_identity(
    tx: &mut Transaction<'_, Postgres>,
    current_user_id: Uuid,
    identity: &SocialIdentity,
) -> Result<(), AppError> {
    let current_user = load_social_user_by_id(tx, current_user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User no longer exists".to_string()))?;

    if !current_user.is_active {
        return Err(AppError::Forbidden("Account is suspended".to_string()));
    }

    if let Some(existing_provider_id) =
        load_existing_provider_link(tx, current_user.id, identity.provider_name).await?
    {
        if existing_provider_id != identity.provider_id.as_str() {
            return Err(AppError::Conflict(format!(
                "This account is already linked to a different {} account.",
                identity.provider_label
            )));
        }

        maybe_mark_user_verified_from_social(tx, &current_user, &identity.email).await?;
        return Ok(());
    }

    if let Some(linked_user) =
        load_social_user_by_provider(tx, identity.provider_name, &identity.provider_id).await?
    {
        if linked_user.id != current_user.id {
            return Err(AppError::Conflict(format!(
                "This {} account is already linked to another user.",
                identity.provider_label
            )));
        }

        maybe_mark_user_verified_from_social(tx, &current_user, &identity.email).await?;
        return Ok(());
    }

    insert_provider_link(
        tx,
        current_user.id,
        identity.provider_name,
        identity.provider_label,
        &identity.provider_id,
    )
    .await?;

    maybe_mark_user_verified_from_social(tx, &current_user, &identity.email).await?;

    Ok(())
}

async fn issue_tokens_for_user(
    pool: &PgPool,
    user: AuthUserRecord,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let access_token = jwt::create_access_token(
        user.id,
        &user.role,
        &config.jwt_secret,
        config.jwt_access_expiry_minutes,
    )?;
    let refresh_token = create_refresh_token(pool, user.id, config).await?;

    Ok(AuthTokens {
        access_token,
        refresh_token,
        user: user.into(),
    })
}

async fn fetch_google_identity(
    token: &str,
    expected_nonce: Option<&str>,
    config: &AppConfig,
) -> Result<SocialIdentity, AppError> {
    let client = reqwest::Client::new();
    let res = client
        .get("https://oauth2.googleapis.com/tokeninfo")
        .query(&[("id_token", token)])
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Failed to verify Google token: {}", e)))?;

    if !res.status().is_success() {
        return Err(AppError::Unauthorized("Invalid Google token".to_string()));
    }

    let body_text = res
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read Google token response: {}", e)))?;

    let token_info: GoogleTokenInfo = serde_json::from_str(&body_text).map_err(|e| {
        tracing::error!(error = %e, body = %body_text, "Failed to parse Google token info");
        AppError::Unauthorized("Failed to parse Google token info".to_string())
    })?;

    if token_info.aud != config.google_client_id {
        return Err(AppError::Unauthorized(
            "Token was not issued for this app".to_string(),
        ));
    }

    let is_email_verified = match token_info.email_verified {
        serde_json::Value::Bool(b) => b,
        serde_json::Value::String(ref s) => s == "true",
        _ => false,
    };

    if !is_email_verified {
        return Err(AppError::Unauthorized(
            "Google email is not verified".to_string(),
        ));
    }

    if let Some(expected) = expected_nonce
        && token_info.nonce.as_deref() != Some(expected)
    {
        return Err(AppError::Unauthorized("Invalid ID Token nonce".to_string()));
    }

    let email = normalize_email(&token_info.email)?;
    let name = coerce_oauth_user_name(token_info.name.as_deref(), &email);
    let image = normalize_optional_image(token_info.picture.as_deref());
    Ok(SocialIdentity {
        provider_name: GOOGLE_PROVIDER_NAME,
        provider_label: "Google",
        provider_id: token_info.sub,
        email,
        name,
        image,
    })
}

async fn authenticate_social_identity(
    pool: &PgPool,
    identity: &SocialIdentity,
) -> Result<AuthUserRecord, AppError> {
    let mut tx = pool.begin().await?;
    let user = resolve_social_user(&mut tx, identity).await?;
    tx.commit().await?;
    Ok(user)
}

async fn google_authenticate(
    pool: &PgPool,
    token: &str,
    expected_nonce: Option<&str>,
    config: &AppConfig,
) -> Result<AuthUserRecord, AppError> {
    let identity = fetch_google_identity(token, expected_nonce, config).await?;
    authenticate_social_identity(pool, &identity).await
}

async fn google_identity_with_authorization_code(
    code: &str,
    redirect_uri: &str,
    nonce: Option<&str>,
    code_verifier: &str,
    config: &AppConfig,
) -> Result<SocialIdentity, AppError> {
    let id_token =
        exchange_google_authorization_code(code, redirect_uri, code_verifier, config).await?;
    fetch_google_identity(&id_token, nonce, config).await
}

pub(crate) async fn google_authenticate_with_authorization_code(
    pool: &PgPool,
    code: &str,
    redirect_uri: &str,
    nonce: Option<&str>,
    code_verifier: &str,
    config: &AppConfig,
) -> Result<AuthUserRecord, AppError> {
    let identity =
        google_identity_with_authorization_code(code, redirect_uri, nonce, code_verifier, config)
            .await?;
    authenticate_social_identity(pool, &identity).await
}

pub(crate) async fn link_google_account_with_authorization_code(
    pool: &PgPool,
    current_user_id: Uuid,
    code: &str,
    redirect_uri: &str,
    nonce: Option<&str>,
    code_verifier: &str,
    config: &AppConfig,
) -> Result<(), AppError> {
    let identity =
        google_identity_with_authorization_code(code, redirect_uri, nonce, code_verifier, config)
            .await?;
    let mut tx = pool.begin().await?;
    link_social_identity(&mut tx, current_user_id, &identity).await?;
    tx.commit().await?;
    Ok(())
}

async fn fetch_github_identity(code: &str, config: &AppConfig) -> Result<SocialIdentity, AppError> {
    let client = reqwest::Client::new();

    let params = [
        ("client_id", config.github_client_id.as_str()),
        ("client_secret", config.github_client_secret.as_str()),
        ("code", code),
    ];

    let token_res = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Failed to retrieve GitHub token: {}", e)))?;

    if !token_res.status().is_success() {
        return Err(AppError::Unauthorized(
            "Invalid GitHub authorization code".to_string(),
        ));
    }

    let token_data: GithubTokenResponse = token_res.json().await.map_err(|_| {
        AppError::Unauthorized(
            "Failed to parse GitHub token response. The code might be expired or invalid."
                .to_string(),
        )
    })?;

    let github_access_token = token_data.access_token;

    let user_res = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", github_access_token))
        .header("User-Agent", "rust-ecommerce-backend")
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Failed to fetch GitHub profile: {}", e)))?;

    if !user_res.status().is_success() {
        return Err(AppError::Unauthorized(
            "Failed to fetch GitHub profile".to_string(),
        ));
    }

    let github_user: GithubUser = user_res
        .json()
        .await
        .map_err(|_| AppError::Internal("Failed to parse GitHub profile".to_string()))?;

    let emails_res = client
        .get("https://api.github.com/user/emails")
        .header("Authorization", format!("Bearer {}", github_access_token))
        .header("User-Agent", "rust-ecommerce-backend")
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Failed to fetch GitHub emails: {}", e)))?;

    if !emails_res.status().is_success() {
        return Err(AppError::Unauthorized(
            "Failed to fetch GitHub emails".to_string(),
        ));
    }

    let emails: Vec<GithubEmail> = emails_res
        .json()
        .await
        .map_err(|_| AppError::Internal("Failed to parse GitHub emails".to_string()))?;

    let primary_email = emails
        .into_iter()
        .find(|e| e.primary && e.verified)
        .map(|e| e.email)
        .ok_or_else(|| {
            AppError::Unauthorized("No primary, verified email found on GitHub".to_string())
        })?;
    let primary_email = normalize_email(&primary_email)?;
    let name = coerce_oauth_user_name(github_user.name.as_deref(), &primary_email);
    let image = normalize_optional_image(github_user.avatar_url.as_deref());

    Ok(SocialIdentity {
        provider_name: GITHUB_PROVIDER_NAME,
        provider_label: "GitHub",
        provider_id: github_user.id.to_string(),
        email: primary_email,
        name,
        image,
    })
}

/// Authenticate user via Google ID Token
pub async fn google_login(
    pool: &PgPool,
    token: &str,
    expected_nonce: Option<&str>,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let user = google_authenticate(pool, token, expected_nonce, config).await?;
    issue_tokens_for_user(pool, user, config).await
}

pub(crate) async fn github_authenticate(
    pool: &PgPool,
    code: &str,
    config: &AppConfig,
) -> Result<AuthUserRecord, AppError> {
    let identity = fetch_github_identity(code, config).await?;
    authenticate_social_identity(pool, &identity).await
}

pub(crate) async fn link_github_account(
    pool: &PgPool,
    current_user_id: Uuid,
    code: &str,
    config: &AppConfig,
) -> Result<(), AppError> {
    let identity = fetch_github_identity(code, config).await?;
    let mut tx = pool.begin().await?;
    link_social_identity(&mut tx, current_user_id, &identity).await?;
    tx.commit().await?;
    Ok(())
}

/// Authenticate user via GitHub Authorization Code
pub async fn github_login(
    pool: &PgPool,
    code: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let user = github_authenticate(pool, code, config).await?;
    issue_tokens_for_user(pool, user, config).await
}

pub async fn create_oauth_login_ticket(
    pool: &PgPool,
    user_id: Uuid,
    config: &AppConfig,
) -> Result<String, AppError> {
    let ticket = jwt::create_refresh_token();
    let ticket_hash = hash_oauth_login_ticket(&config.jwt_secret, &ticket);
    let expires_at = Utc::now() + Duration::minutes(OAUTH_LOGIN_TICKET_EXPIRY_MINUTES);

    sqlx::query(
        r#"INSERT INTO oauth_login_tickets
           (id, ticket_hash, user_id, expires_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(Uuid::now_v7())
    .bind(ticket_hash)
    .bind(user_id)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(ticket)
}

pub async fn exchange_oauth_login_ticket(
    pool: &PgPool,
    ticket: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let ticket_hash = hash_oauth_login_ticket(&config.jwt_secret, ticket);
    let mut tx = pool.begin().await?;

    let record = sqlx::query_as::<_, (Uuid, chrono::DateTime<Utc>)>(
        r#"DELETE FROM oauth_login_tickets
           WHERE ticket_hash = $1
           RETURNING user_id, expires_at"#,
    )
    .bind(&ticket_hash)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| {
        AppError::Unauthorized("OAuth login ticket is invalid or expired".to_string())
    })?;

    let (user_id, expires_at) = record;

    if Utc::now() > expires_at {
        tx.commit().await?;
        return Err(AppError::Unauthorized(
            "OAuth login ticket has expired".to_string(),
        ));
    }

    let user = sqlx::query_as::<_, AuthUserRecord>(
        r#"SELECT id, name, email, image, role
           FROM users
           WHERE id = $1 AND is_active = TRUE"#,
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::Unauthorized("User not found or inactive".to_string()))?;

    tx.commit().await?;

    issue_tokens_for_user(pool, user, config).await
}

/// Rotate refresh token: validate old one, delete it, create new one
pub async fn rotate_refresh_token(
    pool: &PgPool,
    old_token: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let old_token_hash = hash_refresh_token(&config.jwt_secret, old_token);
    let mut tx = pool.begin().await?;

    let record = sqlx::query_as::<_, (Uuid, chrono::DateTime<Utc>)>(
        r#"DELETE FROM refresh_tokens
           WHERE token_hash = $1
           RETURNING user_id, expires_at"#,
    )
    .bind(&old_token_hash)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid refresh token".to_string()))?;

    let (user_id, expires_at) = record;

    if Utc::now() > expires_at {
        tx.commit().await?;
        return Err(AppError::Unauthorized(
            "Refresh token has expired".to_string(),
        ));
    }

    let user = sqlx::query_as::<_, (Uuid, String, String, Option<String>, String)>(
        "SELECT id, name, email, image, role FROM users WHERE id = $1 AND is_active = TRUE",
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::Unauthorized("User not found or inactive".to_string()))?;

    let (uid, name, email, image, role) = user;

    let access_token = jwt::create_access_token(
        uid,
        &role,
        &config.jwt_secret,
        config.jwt_access_expiry_minutes,
    )?;
    let new_refresh_token = create_refresh_token_in_tx(&mut tx, uid, config).await?;

    tx.commit().await?;

    Ok(AuthTokens {
        access_token,
        refresh_token: new_refresh_token,
        user: UserInfo {
            id: uid,
            name,
            email,
            image,
            role,
        },
    })
}

/// Logout: delete refresh token
pub async fn logout(
    pool: &PgPool,
    refresh_token: &str,
    config: &AppConfig,
) -> Result<(), AppError> {
    let token_hash = hash_refresh_token(&config.jwt_secret, refresh_token);

    sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
        .bind(token_hash)
        .execute(pool)
        .await?;

    Ok(())
}

async fn replace_email_verification(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
    purpose: &str,
    user_id: Option<Uuid>,
    snapshot: VerificationSnapshot<'_>,
    code_hash: &str,
    expires_at: chrono::DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM email_verifications WHERE email = $1 AND purpose = $2")
        .bind(email)
        .bind(purpose)
        .execute(&mut **tx)
        .await?;

    if let Some(user_id) = user_id {
        sqlx::query("DELETE FROM email_verifications WHERE user_id = $1 AND purpose = $2")
            .bind(user_id)
            .bind(purpose)
            .execute(&mut **tx)
            .await?;
    }

    sqlx::query(
        r#"INSERT INTO email_verifications
           (id, email, name, image, code_hash, password_hash, user_id, purpose, expires_at, attempt_count)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0)"#,
    )
    .bind(Uuid::now_v7())
    .bind(email)
    .bind(snapshot.name)
    .bind(snapshot.image)
    .bind(code_hash)
    .bind(snapshot.password_hash)
    .bind(user_id)
    .bind(purpose)
    .bind(expires_at)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn load_and_validate_email_verification(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
    purpose: &str,
    user_id: Option<Uuid>,
    code: &str,
    secret: &str,
) -> Result<EmailVerificationRecord, AppError> {
    let record = sqlx::query_as::<_, EmailVerificationRecord>(
        r#"SELECT id, email, name, image, code_hash, password_hash, expires_at, attempt_count
           FROM email_verifications
           WHERE email = $1 AND purpose = $2 AND user_id IS NOT DISTINCT FROM $3
           ORDER BY created_at DESC
           LIMIT 1
           FOR UPDATE"#,
    )
    .bind(email)
    .bind(purpose)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        AppError::BadRequest("No pending verification found for this email".to_string())
    })?;

    if Utc::now() > record.expires_at {
        delete_email_verification(tx, record.id).await?;
        return Err(AppError::BadRequest(
            "Verification code has expired".to_string(),
        ));
    }

    if record.attempt_count >= MAX_EMAIL_VERIFICATION_ATTEMPTS {
        delete_email_verification(tx, record.id).await?;
        return Err(AppError::TooManyRequests(
            "Too many verification attempts. Please request a new code.".to_string(),
        ));
    }

    let expected_hash = hash_verification_code(secret, purpose, email, code);
    if expected_hash != record.code_hash {
        let next_attempt_count = record.attempt_count + 1;

        if next_attempt_count >= MAX_EMAIL_VERIFICATION_ATTEMPTS {
            delete_email_verification(tx, record.id).await?;
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

async fn delete_email_verification(
    tx: &mut Transaction<'_, Postgres>,
    verification_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM email_verifications WHERE id = $1")
        .bind(verification_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

async fn delete_refresh_tokens_for_user_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM refresh_tokens WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

async fn ensure_password_is_new(
    existing_password_hash: Option<&str>,
    next_password: &str,
) -> Result<(), AppError> {
    let Some(existing_password_hash) = existing_password_hash else {
        return Ok(());
    };

    let is_same = password::verify_password(
        next_password.to_string(),
        existing_password_hash.to_string(),
    )
    .await?;

    if is_same {
        return Err(AppError::BadRequest(
            "New password must be different from the current password".to_string(),
        ));
    }

    Ok(())
}

/// Internal: create and store a refresh token in DB
async fn create_refresh_token(
    pool: &PgPool,
    user_id: Uuid,
    config: &AppConfig,
) -> Result<String, AppError> {
    let (id, token, token_hash, expires_at) = build_refresh_token_record(config);

    sqlx::query(
        r#"INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(id)
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(token)
}

async fn create_refresh_token_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    config: &AppConfig,
) -> Result<String, AppError> {
    let (id, token, token_hash, expires_at) = build_refresh_token_record(config);

    sqlx::query(
        r#"INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(id)
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(&mut **tx)
    .await?;

    Ok(token)
}

fn build_refresh_token_record(config: &AppConfig) -> (Uuid, String, String, chrono::DateTime<Utc>) {
    let id = Uuid::now_v7();
    let token = jwt::create_refresh_token();
    let token_hash = hash_refresh_token(&config.jwt_secret, &token);
    let expires_at = Utc::now() + Duration::days(config.jwt_refresh_expiry_days);

    (id, token, token_hash, expires_at)
}

async fn exchange_google_authorization_code(
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
    config: &AppConfig,
) -> Result<String, AppError> {
    let client = reqwest::Client::new();

    let token_res = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", config.google_client_id.as_str()),
            ("client_secret", config.google_client_secret.as_str()),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Failed to retrieve Google token: {}", e)))?;

    if !token_res.status().is_success() {
        return Err(AppError::Unauthorized(
            "Invalid Google authorization code".to_string(),
        ));
    }

    let token_data: GoogleAuthorizationCodeResponse = token_res.json().await.map_err(|_| {
        AppError::Unauthorized(
            "Failed to parse Google token response. The code might be expired or invalid."
                .to_string(),
        )
    })?;

    Ok(token_data.id_token)
}
