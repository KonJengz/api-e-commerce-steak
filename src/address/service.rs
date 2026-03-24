use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::shared::errors::AppError;

use super::model::{Address, CreateAddressRequest, UpdateAddressRequest};

/// List all addresses for a user
pub async fn list_addresses(pool: &PgPool, user_id: Uuid) -> Result<Vec<Address>, AppError> {
    let addresses = sqlx::query_as::<_, Address>(
        r#"SELECT id, user_id, recipient_name, phone, address_line, city, postal_code, country, is_default, created_at
           FROM addresses WHERE user_id = $1 ORDER BY is_default DESC, created_at DESC"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(addresses)
}

/// Create a new address
pub async fn create_address(
    pool: &PgPool,
    user_id: Uuid,
    req: &CreateAddressRequest,
) -> Result<Address, AppError> {
    let id = Uuid::now_v7();
    let is_default = req.is_default.unwrap_or(false);
    let country = req
        .country
        .clone()
        .unwrap_or_else(|| "Thailand".to_string());
    let mut tx = pool.begin().await?;

    // If this is set as default, unset other defaults
    if is_default {
        sqlx::query("UPDATE addresses SET is_default = FALSE WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }

    let address = sqlx::query_as::<_, Address>(
        r#"INSERT INTO addresses (id, user_id, recipient_name, phone, address_line, city, postal_code, country, is_default, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           RETURNING id, user_id, recipient_name, phone, address_line, city, postal_code, country, is_default, created_at"#,
    )
    .bind(id)
    .bind(user_id)
    .bind(&req.recipient_name)
    .bind(&req.phone)
    .bind(&req.address_line)
    .bind(&req.city)
    .bind(&req.postal_code)
    .bind(&country)
    .bind(is_default)
    .bind(Utc::now())
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(address)
}

/// Update an address (only if it belongs to the user)
pub async fn update_address(
    pool: &PgPool,
    user_id: Uuid,
    address_id: Uuid,
    req: &UpdateAddressRequest,
) -> Result<Address, AppError> {
    let mut tx = pool.begin().await?;

    // Verify ownership
    let _existing = sqlx::query_as::<_, Address>(
        r#"SELECT id, user_id, recipient_name, phone, address_line, city, postal_code, country, is_default, created_at
           FROM addresses WHERE id = $1 AND user_id = $2
           FOR UPDATE"#,
    )
    .bind(address_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Address not found".to_string()))?;

    // If setting as default, unset others
    if req.is_default == Some(true) {
        sqlx::query("UPDATE addresses SET is_default = FALSE WHERE user_id = $1 AND id != $2")
            .bind(user_id)
            .bind(address_id)
            .execute(&mut *tx)
            .await?;
    }

    let address = sqlx::query_as::<_, Address>(
        r#"UPDATE addresses SET
            recipient_name = COALESCE($1, recipient_name),
            phone = COALESCE($2, phone),
            address_line = COALESCE($3, address_line),
            city = COALESCE($4, city),
            postal_code = COALESCE($5, postal_code),
            country = COALESCE($6, country),
            is_default = COALESCE($7, is_default)
           WHERE id = $8 AND user_id = $9
           RETURNING id, user_id, recipient_name, phone, address_line, city, postal_code, country, is_default, created_at"#,
    )
    .bind(&req.recipient_name)
    .bind(&req.phone)
    .bind(&req.address_line)
    .bind(&req.city)
    .bind(&req.postal_code)
    .bind(&req.country)
    .bind(req.is_default)
    .bind(address_id)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(address)
}

/// Delete an address (only if it belongs to the user)
pub async fn delete_address(
    pool: &PgPool,
    user_id: Uuid,
    address_id: Uuid,
) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM addresses WHERE id = $1 AND user_id = $2")
        .bind(address_id)
        .bind(user_id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Address not found".to_string()));
    }

    Ok(())
}
