use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::shared::errors::AppError;
use crate::shared::jwt;
use crate::shared::password;

use super::model::{AuthTokens, MessageResponse, UserInfo};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GoogleTokenInfo {
    aud: String,
    email: String,
    sub: String,
}

#[derive(Debug, Deserialize)]
struct GithubTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct GithubUser {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct GithubEmail {
    email: String,
    primary: bool,
    verified: bool,
}

/// Create email verification record and return verification code
pub async fn create_email_verification(
    pool: &PgPool,
    email: &str,
    plain_password: &str,
) -> Result<String, AppError> {
    // Check if user already exists
    let existing = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await?;

    if existing > 0 {
        return Err(AppError::Conflict("Email already registered".to_string()));
    }

    // Delete any existing verification for this email
    sqlx::query("DELETE FROM email_verifications WHERE email = $1")
        .bind(email)
        .execute(pool)
        .await?;

    let id = Uuid::now_v7();
    let code = jwt::generate_verification_code();
    let hashed_password = password::hash_password(plain_password)?;
    let expires_at = Utc::now() + Duration::minutes(15);

    sqlx::query(
        r#"INSERT INTO email_verifications (id, email, code, password_hash, expires_at)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(id)
    .bind(email)
    .bind(&code)
    .bind(&hashed_password)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(code)
}

/// Verify email code and create user (does NOT return tokens — user must login separately)
pub async fn verify_email_and_create_user(
    pool: &PgPool,
    email: &str,
    code: &str,
) -> Result<MessageResponse, AppError> {
    // Find verification record
    let record = sqlx::query_as::<_, (Uuid, String, String, Option<String>, chrono::DateTime<Utc>)>(
        r#"SELECT id, email, code, password_hash, expires_at
           FROM email_verifications
           WHERE email = $1 AND code = $2"#,
    )
    .bind(email)
    .bind(code)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::BadRequest("Invalid verification code".to_string()))?;

    let (verification_id, _email, _code, password_hash, expires_at) = record;

    // Check expiry
    if Utc::now() > expires_at {
        // Clean up expired record
        sqlx::query("DELETE FROM email_verifications WHERE id = $1")
            .bind(verification_id)
            .execute(pool)
            .await?;
        return Err(AppError::BadRequest("Verification code has expired".to_string()));
    }

    // Create user
    let user_id = Uuid::now_v7();
    let now = Utc::now();

    sqlx::query(
        r#"INSERT INTO users (id, email, password_hash, role, is_active, is_verified, created_at, updated_at)
           VALUES ($1, $2, $3, 'USER', TRUE, TRUE, $4, $4)"#,
    )
    .bind(user_id)
    .bind(email)
    .bind(&password_hash)
    .bind(now)
    .execute(pool)
    .await?;

    // Delete verification record
    sqlx::query("DELETE FROM email_verifications WHERE id = $1")
        .bind(verification_id)
        .execute(pool)
        .await?;

    Ok(MessageResponse {
        message: "Email verified successfully. Please login to continue.".to_string(),
    })
}

/// Authenticate user with email/password and return tokens
pub async fn login(
    pool: &PgPool,
    email: &str,
    plain_password: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let user = sqlx::query_as::<_, (Uuid, String, Option<String>, String, bool, bool)>(
        r#"SELECT id, email, password_hash, role, is_active, is_verified
           FROM users WHERE email = $1"#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid email or password".to_string()))?;

    let (user_id, user_email, password_hash, role, is_active, is_verified) = user;

    if !is_active {
        return Err(AppError::Forbidden("Account is suspended".to_string()));
    }

    if !is_verified {
        return Err(AppError::Forbidden("Email not verified".to_string()));
    }

    let password_hash = password_hash
        .ok_or_else(|| AppError::Unauthorized("Please login with your social account".to_string()))?;

    let is_valid = password::verify_password(plain_password, &password_hash)?;
    if !is_valid {
        return Err(AppError::Unauthorized("Invalid email or password".to_string()));
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
            email: user_email,
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
    // 1. Verify token with Google's tokeninfo endpoint
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

    // 2. Verify Client ID
    if token_info.aud != config.google_client_id {
        return Err(AppError::Unauthorized("Token was not issued for this app".to_string()));
    }

    let email = token_info.email;
    let google_id = token_info.sub;

    let mut tx = pool.begin().await?;

    // 3. Find or Create User
    let user = sqlx::query_as::<_, (Uuid, String, bool)>(
        r#"SELECT id, role, is_active FROM users WHERE email = $1"#
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;

    let (user_id, role, _is_active) = match user {
        Some((id, role, is_active)) => {
            if !is_active {
                return Err(AppError::Forbidden("Account is suspended".to_string()));
            }
            (id, role, is_active)
        }
        None => {
            // Create new user for social login
            let new_user_id = Uuid::now_v7();
            let now = Utc::now();
            
            sqlx::query(
                r#"INSERT INTO users (id, email, password_hash, role, is_active, is_verified, created_at, updated_at)
                   VALUES ($1, $2, NULL, 'USER', TRUE, TRUE, $3, $3)"#
            )
            .bind(new_user_id)
            .bind(&email)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            (new_user_id, "USER".to_string(), true)
        }
    };

    // 4. Upsert account_providers record
    sqlx::query(
        r#"INSERT INTO account_providers (id, user_id, provider_name, provider_id)
           VALUES ($1, $2, 'google', $3)
           ON CONFLICT (provider_name, provider_id) DO NOTHING"#
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(&google_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // 5. Generate tokens
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
            email,
            role,
        },
    })
}

/// Authenticate user via GitHub Authorization Code
pub async fn github_login(
    pool: &PgPool,
    code: &str,
    config: &AppConfig,
) -> Result<AuthTokens, AppError> {
    let client = reqwest::Client::new();

    // 1. Exchange code for access token
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
        return Err(AppError::Unauthorized("Invalid GitHub authorization code".to_string()));
    }

    let token_data: GithubTokenResponse = token_res
        .json()
        .await
        .map_err(|_| AppError::Unauthorized("Failed to parse GitHub token response. The code might be expired or invalid.".to_string()))?;

    let github_access_token = token_data.access_token;

    // 2. Fetch User Profile
    let user_res = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", github_access_token))
        .header("User-Agent", "rust-ecommerce-backend")
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Failed to fetch GitHub profile: {}", e)))?;

    if !user_res.status().is_success() {
        return Err(AppError::Unauthorized("Failed to fetch GitHub profile".to_string()));
    }

    let github_user: GithubUser = user_res
        .json()
        .await
        .map_err(|_| AppError::Internal("Failed to parse GitHub profile".to_string()))?;

    let github_id_str = github_user.id.to_string();

    // 3. Fetch User Emails (to find the primary verified one)
    let emails_res = client
        .get("https://api.github.com/user/emails")
        .header("Authorization", format!("Bearer {}", github_access_token))
        .header("User-Agent", "rust-ecommerce-backend")
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Failed to fetch GitHub emails: {}", e)))?;

    if !emails_res.status().is_success() {
        return Err(AppError::Unauthorized("Failed to fetch GitHub emails".to_string()));
    }

    let emails: Vec<GithubEmail> = emails_res
        .json()
        .await
        .map_err(|_| AppError::Internal("Failed to parse GitHub emails".to_string()))?;

    let primary_email = emails
        .into_iter()
        .find(|e| e.primary && e.verified)
        .map(|e| e.email)
        .ok_or_else(|| AppError::Unauthorized("No primary, verified email found on GitHub".to_string()))?;

    let mut tx = pool.begin().await?;

    // 4. Find or Create User
    let user = sqlx::query_as::<_, (Uuid, String, bool)>(
        r#"SELECT id, role, is_active FROM users WHERE email = $1"#
    )
    .bind(&primary_email)
    .fetch_optional(&mut *tx)
    .await?;

    let (user_id, role, _is_active) = match user {
        Some((id, role, is_active)) => {
            if !is_active {
                return Err(AppError::Forbidden("Account is suspended".to_string()));
            }
            (id, role, is_active)
        }
        None => {
            // Create new user for social login
            let new_user_id = Uuid::now_v7();
            let now = Utc::now();
            
            sqlx::query(
                r#"INSERT INTO users (id, email, password_hash, role, is_active, is_verified, created_at, updated_at)
                   VALUES ($1, $2, NULL, 'USER', TRUE, TRUE, $3, $3)"#
            )
            .bind(new_user_id)
            .bind(&primary_email)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            (new_user_id, "USER".to_string(), true)
        }
    };

    // 5. Upsert account_providers record
    sqlx::query(
        r#"INSERT INTO account_providers (id, user_id, provider_name, provider_id)
           VALUES ($1, $2, 'github', $3)
           ON CONFLICT (provider_name, provider_id) DO NOTHING"#
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(&github_id_str)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // 6. Generate tokens
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
            email: primary_email,
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
    // Find and validate the refresh token
    let record = sqlx::query_as::<_, (Uuid, Uuid, chrono::DateTime<Utc>)>(
        r#"SELECT id, user_id, expires_at FROM refresh_tokens WHERE token = $1"#,
    )
    .bind(old_token)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid refresh token".to_string()))?;

    let (token_id, user_id, expires_at) = record;

    // Check expiry
    if Utc::now() > expires_at {
        sqlx::query("DELETE FROM refresh_tokens WHERE id = $1")
            .bind(token_id)
            .execute(pool)
            .await?;
        return Err(AppError::Unauthorized("Refresh token has expired".to_string()));
    }

    // Delete old token
    sqlx::query("DELETE FROM refresh_tokens WHERE id = $1")
        .bind(token_id)
        .execute(pool)
        .await?;

    // Get user info
    let user = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id, email, role FROM users WHERE id = $1 AND is_active = TRUE",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("User not found or inactive".to_string()))?;

    let (uid, email, role) = user;

    // Create new tokens
    let access_token = jwt::create_access_token(
        uid,
        &role,
        &config.jwt_secret,
        config.jwt_access_expiry_minutes,
    )?;
    let new_refresh_token = create_refresh_token(pool, uid, config).await?;

    Ok(AuthTokens {
        access_token,
        refresh_token: new_refresh_token,
        user: UserInfo {
            id: uid,
            email,
            role,
        },
    })
}

/// Logout: delete refresh token
pub async fn logout(pool: &PgPool, refresh_token: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM refresh_tokens WHERE token = $1")
        .bind(refresh_token)
        .execute(pool)
        .await?;

    Ok(())
}

/// Internal: create and store a refresh token in DB
async fn create_refresh_token(
    pool: &PgPool,
    user_id: Uuid,
    config: &AppConfig,
) -> Result<String, AppError> {
    let id = Uuid::now_v7();
    let token = jwt::create_refresh_token();
    let expires_at = Utc::now() + Duration::days(config.jwt_refresh_expiry_days);

    sqlx::query(
        r#"INSERT INTO refresh_tokens (id, user_id, token, expires_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(id)
    .bind(user_id)
    .bind(&token)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(token)
}
