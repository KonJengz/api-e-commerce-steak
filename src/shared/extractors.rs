use axum::{extract::FromRequestParts, http::request::Parts};
use uuid::Uuid;

use crate::shared::errors::AppError;
use crate::shared::jwt;
use crate::AppState;

/// Extracted authenticated user from JWT in Authorization header
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub role: String,
}

/// Admin-only extractor — rejects non-ADMIN users
#[derive(Debug, Clone)]
pub struct AdminUser {
    pub user_id: Uuid,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing authorization header".to_string()))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::Unauthorized("Invalid authorization format".to_string()))?;

        let claims = jwt::verify_access_token(token, &state.config.jwt_secret)?;
        let current_user =
            sqlx::query_as::<_, (String, bool)>("SELECT role, is_active FROM users WHERE id = $1")
                .bind(claims.sub)
                .fetch_optional(&state.pool)
                .await?
                .ok_or_else(|| AppError::Unauthorized("User no longer exists".to_string()))?;

        let (role, is_active) = current_user;

        if !is_active {
            return Err(AppError::Unauthorized(
                "User account is inactive".to_string(),
            ));
        }

        Ok(AuthUser {
            user_id: claims.sub,
            role,
        })
    }
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_user = AuthUser::from_request_parts(parts, state).await?;

        if auth_user.role != "ADMIN" {
            return Err(AppError::Forbidden("Admin access required".to_string()));
        }

        Ok(AdminUser {
            user_id: auth_user.user_id,
        })
    }
}
