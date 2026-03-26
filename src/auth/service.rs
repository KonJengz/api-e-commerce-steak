use chrono::{Duration, Utc};
use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::shared::errors::AppError;
use crate::shared::jwt;
use crate::shared::password;
use crate::shared::security::{
    EMAIL_VERIFICATION_PURPOSE_REGISTER, coerce_oauth_user_name, fallback_user_name,
    hash_refresh_token, hash_verification_code, normalize_email, normalize_optional_image,
    normalize_required_name,
};

use super::model::{AuthTokens, UserInfo};

const EMAIL_VERIFICATION_EXPIRY_MINUTES: i64 = 15;
const MAX_EMAIL_VERIFICATION_ATTEMPTS: i32 = 5;
const OAUTH_LOGIN_TICKET_EXPIRY_MINUTES: i64 = 5;

#[derive(Debug, Deserialize)]
struct GoogleTokenInfo {
    aud: String,
    email: String,
    name: Option<String>,
    picture: Option<String>,
    sub: String,
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
    let hashed_password = password::hash_password(plain_password.to_string()).await?;
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

/// Authenticate user via Google ID Token
pub async fn google_login(
    pool: &PgPool,
    token: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
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

    let token_info: GoogleTokenInfo = res
        .json()
        .await
        .map_err(|_| AppError::Unauthorized("Failed to parse Google token info".to_string()))?;

    if token_info.aud != config.google_client_id {
        return Err(AppError::Unauthorized(
            "Token was not issued for this app".to_string(),
        ));
    }

    let email = normalize_email(&token_info.email)?;
    let name = coerce_oauth_user_name(token_info.name.as_deref(), &email);
    let image = normalize_optional_image(token_info.picture.as_deref());
    let google_id = token_info.sub;

    let mut tx = pool.begin().await?;

    let user = sqlx::query_as::<_, (Uuid, String, Option<String>, String, bool)>(
        r#"SELECT id, name, image, role, is_active FROM users WHERE email = $1"#,
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;

    let (user_id, user_name, user_image, role, _is_active) = match user {
        Some((id, existing_name, existing_image, role, is_active)) => {
            if !is_active {
                return Err(AppError::Forbidden("Account is suspended".to_string()));
            }
            // Auto-verify the email if they log in via Google
            sqlx::query(
                "UPDATE users SET is_verified = TRUE WHERE id = $1 AND is_verified = FALSE",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
            (id, existing_name, existing_image, role, is_active)
        }
        None => {
            let new_user_id = Uuid::now_v7();
            let now = Utc::now();

            sqlx::query(
                r#"INSERT INTO users (id, name, email, image, password_hash, role, is_active, is_verified, created_at, updated_at)
                   VALUES ($1, $2, $3, $4, NULL, 'USER', TRUE, TRUE, $5, $5)"#,
            )
            .bind(new_user_id)
            .bind(&name)
            .bind(&email)
            .bind(image.as_deref())
            .bind(now)
            .execute(&mut *tx)
            .await?;

            (new_user_id, name, image, "USER".to_string(), true)
        }
    };

    sqlx::query(
        r#"INSERT INTO account_providers (id, user_id, provider_name, provider_id)
           VALUES ($1, $2, 'google', $3)
           ON CONFLICT (provider_name, provider_id) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(&google_id)
    .execute(&mut *tx)
    .await?;

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
            email,
            image: user_image,
            role,
        },
    })
}

pub async fn google_login_with_authorization_code(
    pool: &PgPool,
    code: &str,
    redirect_uri: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let id_token = exchange_google_authorization_code(code, redirect_uri, config).await?;
    google_login(pool, &id_token, config).await
}

/// Authenticate user via GitHub Authorization Code
pub async fn github_login(
    pool: &PgPool,
    code: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
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

    let github_id_str = github_user.id.to_string();

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

    let mut tx = pool.begin().await?;

    let user = sqlx::query_as::<_, (Uuid, String, Option<String>, String, bool)>(
        r#"SELECT id, name, image, role, is_active FROM users WHERE email = $1"#,
    )
    .bind(&primary_email)
    .fetch_optional(&mut *tx)
    .await?;

    let (user_id, user_name, user_image, role, _is_active) = match user {
        Some((id, existing_name, existing_image, role, is_active)) => {
            if !is_active {
                return Err(AppError::Forbidden("Account is suspended".to_string()));
            }
            // Auto-verify the email if they log in via GitHub
            sqlx::query(
                "UPDATE users SET is_verified = TRUE WHERE id = $1 AND is_verified = FALSE",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
            (id, existing_name, existing_image, role, is_active)
        }
        None => {
            let new_user_id = Uuid::now_v7();
            let now = Utc::now();

            sqlx::query(
                r#"INSERT INTO users (id, name, email, image, password_hash, role, is_active, is_verified, created_at, updated_at)
                   VALUES ($1, $2, $3, $4, NULL, 'USER', TRUE, TRUE, $5, $5)"#,
            )
            .bind(new_user_id)
            .bind(&name)
            .bind(&primary_email)
            .bind(image.as_deref())
            .bind(now)
            .execute(&mut *tx)
            .await?;

            (new_user_id, name, image, "USER".to_string(), true)
        }
    };

    sqlx::query(
        r#"INSERT INTO account_providers (id, user_id, provider_name, provider_id)
           VALUES ($1, $2, 'github', $3)
           ON CONFLICT (provider_name, provider_id) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(&github_id_str)
    .execute(&mut *tx)
    .await?;

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
            email: primary_email,
            image: user_image,
            role,
        },
    })
}

pub async fn create_oauth_login_ticket(
    pool: &PgPool,
    tokens: &AuthTokens,
) -> Result<String, AppError> {
    let ticket = Uuid::now_v7();
    let expires_at = Utc::now() + Duration::minutes(OAUTH_LOGIN_TICKET_EXPIRY_MINUTES);

    sqlx::query(
        r#"INSERT INTO oauth_login_tickets
           (id, access_token, refresh_token, user_id, expires_at)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(ticket)
    .bind(&tokens.access_token)
    .bind(&tokens.refresh_token)
    .bind(tokens.user.id)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(ticket.to_string())
}

pub async fn exchange_oauth_login_ticket(
    pool: &PgPool,
    ticket: &str,
) -> Result<AuthTokens, AppError> {
    let ticket_id = Uuid::parse_str(ticket)
        .map_err(|_| AppError::BadRequest("Invalid OAuth login ticket".to_string()))?;

    let mut tx = pool.begin().await?;

    let record = sqlx::query_as::<_, (String, String, Uuid, chrono::DateTime<Utc>)>(
        r#"DELETE FROM oauth_login_tickets
           WHERE id = $1
           RETURNING access_token, refresh_token, user_id, expires_at"#,
    )
    .bind(ticket_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| {
        AppError::Unauthorized("OAuth login ticket is invalid or expired".to_string())
    })?;

    let (access_token, refresh_token, user_id, expires_at) = record;

    if Utc::now() > expires_at {
        tx.commit().await?;
        return Err(AppError::Unauthorized(
            "OAuth login ticket has expired".to_string(),
        ));
    }

    let user = sqlx::query_as::<_, (Uuid, String, String, Option<String>, String)>(
        r#"SELECT id, name, email, image, role
           FROM users
           WHERE id = $1 AND is_active = TRUE"#,
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::Unauthorized("User not found or inactive".to_string()))?;

    tx.commit().await?;

    let (id, name, email, image, role) = user;

    Ok(AuthTokens {
        access_token,
        refresh_token,
        user: UserInfo {
            id,
            name,
            email,
            image,
            role,
        },
    })
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
