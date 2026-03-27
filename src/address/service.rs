use chrono::Utc;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::shared::errors::AppError;

use super::model::{Address, CreateAddressRequest, UpdateAddressRequest};

/// List all addresses for a user
pub async fn list_addresses(pool: &PgPool, user_id: Uuid) -> Result<Vec<Address>, AppError> {
    let addresses = sqlx::query_as::<_, Address>(
        r#"SELECT id, user_id, recipient_name, phone, address_line, city, postal_code, is_default, created_at
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
    let mut tx = pool.begin().await?;
    lock_address_owner(&mut tx, user_id).await?;

    let id = Uuid::now_v7();
    let has_existing_addresses = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT id
           FROM addresses
           WHERE user_id = $1
           LIMIT 1
           FOR UPDATE"#,
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let is_default = req.is_default.unwrap_or(false) || has_existing_addresses.is_none();

    // If this is set as default, unset other defaults
    if is_default {
        sqlx::query("UPDATE addresses SET is_default = FALSE WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }

    let address = sqlx::query_as::<_, Address>(
        r#"INSERT INTO addresses (id, user_id, recipient_name, phone, address_line, city, postal_code, is_default, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
           RETURNING id, user_id, recipient_name, phone, address_line, city, postal_code, is_default, created_at"#,
    )
    .bind(id)
    .bind(user_id)
    .bind(&req.recipient_name)
    .bind(&req.phone)
    .bind(&req.address_line)
    .bind(&req.city)
    .bind(&req.postal_code)
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
    lock_address_owner(&mut tx, user_id).await?;

    // Verify ownership
    let existing = sqlx::query_as::<_, Address>(
        r#"SELECT id, user_id, recipient_name, phone, address_line, city, postal_code, is_default, created_at
           FROM addresses WHERE id = $1 AND user_id = $2
           FOR UPDATE"#,
    )
    .bind(address_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Address not found".to_string()))?;

    let fallback_default_id = if req.is_default == Some(false) && existing.is_default {
        select_replacement_address_id(&mut tx, user_id, address_id).await?
    } else {
        None
    };

    let next_is_default = match (req.is_default, existing.is_default, fallback_default_id) {
        (Some(false), true, Some(_)) => Some(false),
        (Some(false), true, None) => Some(true),
        (requested, _, _) => requested,
    };

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
            is_default = COALESCE($6, is_default)
           WHERE id = $7 AND user_id = $8
           RETURNING id, user_id, recipient_name, phone, address_line, city, postal_code, is_default, created_at"#,
    )
    .bind(&req.recipient_name)
    .bind(&req.phone)
    .bind(&req.address_line)
    .bind(&req.city)
    .bind(&req.postal_code)
    .bind(next_is_default)
    .bind(address_id)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    if let Some(fallback_default_id) = fallback_default_id {
        promote_address_to_default(&mut tx, fallback_default_id).await?;
    }

    tx.commit().await?;

    Ok(address)
}

/// Delete an address (only if it belongs to the user)
pub async fn delete_address(
    pool: &PgPool,
    user_id: Uuid,
    address_id: Uuid,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    lock_address_owner(&mut tx, user_id).await?;

    let address = sqlx::query_as::<_, Address>(
        r#"SELECT id, user_id, recipient_name, phone, address_line, city, postal_code, is_default, created_at
           FROM addresses
           WHERE id = $1 AND user_id = $2
           FOR UPDATE"#,
    )
    .bind(address_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Address not found".to_string()))?;

    sqlx::query("DELETE FROM addresses WHERE id = $1 AND user_id = $2")
        .bind(address_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    if address.is_default
        && let Some(fallback_default_id) =
            select_replacement_address_id(&mut tx, user_id, address_id).await?
    {
        promote_address_to_default(&mut tx, fallback_default_id).await?;
    }

    tx.commit().await?;

    Ok(())
}

async fn lock_address_owner(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE id = $1 FOR UPDATE")
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    Ok(())
}

async fn select_replacement_address_id(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    excluded_address_id: Uuid,
) -> Result<Option<Uuid>, AppError> {
    sqlx::query_scalar::<_, Uuid>(
        r#"SELECT id
           FROM addresses
           WHERE user_id = $1 AND id != $2
           ORDER BY created_at DESC, id DESC
           LIMIT 1
           FOR UPDATE"#,
    )
    .bind(user_id)
    .bind(excluded_address_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn promote_address_to_default(
    tx: &mut Transaction<'_, Postgres>,
    address_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("UPDATE addresses SET is_default = TRUE WHERE id = $1")
        .bind(address_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}
