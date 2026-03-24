use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::shared::errors::AppError;
use crate::shared::pagination::{PaginatedResponse, PaginationQuery};

use super::model::{CreateProductRequest, Product, UpdateProductRequest};

/// List all active products
pub async fn list_products(
    pool: &PgPool,
    query: PaginationQuery,
) -> Result<PaginatedResponse<Product>, AppError> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE is_active = TRUE")
        .fetch_one(pool)
        .await?;

    let products = sqlx::query_as::<_, Product>(
        r#"SELECT id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at
           FROM products WHERE is_active = TRUE
           ORDER BY created_at DESC
           LIMIT $1 OFFSET $2"#,
    )
    .bind(query.limit())
    .bind(query.offset())
    .fetch_all(pool)
    .await?;

    Ok(PaginatedResponse::new(
        products,
        total,
        query.page(),
        query.limit(),
    ))
}

/// Get a product by ID
pub async fn get_product(pool: &PgPool, product_id: Uuid) -> Result<Product, AppError> {
    let product = sqlx::query_as::<_, Product>(
        r#"SELECT id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at
           FROM products WHERE id = $1 AND is_active = TRUE"#,
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Product not found".to_string()))?;

    Ok(product)
}

/// Get a product by ID regardless of active status (admin/internal use)
pub async fn get_product_for_admin(pool: &PgPool, product_id: Uuid) -> Result<Product, AppError> {
    let product = sqlx::query_as::<_, Product>(
        r#"SELECT id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at
           FROM products WHERE id = $1"#,
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Product not found".to_string()))?;

    Ok(product)
}

/// Create a product (admin only)
pub async fn create_product(
    pool: &PgPool,
    req: &CreateProductRequest,
    admin_user_id: Uuid,
    config: &AppConfig,
) -> Result<Product, AppError> {
    let id = Uuid::now_v7();
    let now = Utc::now();
    let stock = req.stock.unwrap_or(0);
    let mut tx = pool.begin().await?;

    if let (Some(image_url), Some(image_public_id)) =
        (req.image_url.as_deref(), req.image_public_id.as_deref())
    {
        consume_pending_product_image(
            &mut tx,
            image_public_id,
            admin_user_id,
            image_url,
            config.product_image_upload_ttl_minutes,
        )
        .await?;
    }

    let product = sqlx::query_as::<_, Product>(
        r#"INSERT INTO products (id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, $8, $8)
           RETURNING id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at"#,
    )
    .bind(id)
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.image_url)
    .bind(&req.image_public_id)
    .bind(req.current_price)
    .bind(stock)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(product)
}

/// Update a product (admin only)
pub async fn update_product(
    pool: &PgPool,
    product_id: Uuid,
    req: &UpdateProductRequest,
    admin_user_id: Uuid,
    config: &AppConfig,
) -> Result<Product, AppError> {
    let mut tx = pool.begin().await?;
    let existing_image_public_id = sqlx::query_scalar::<_, Option<String>>(
        "SELECT image_public_id FROM products WHERE id = $1",
    )
    .bind(product_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Product not found".to_string()))?;

    let image_changed = req.image_public_id.as_deref().is_some()
        && req.image_public_id.as_deref() != existing_image_public_id.as_deref();

    if image_changed {
        consume_pending_product_image(
            &mut tx,
            req.image_public_id
                .as_deref()
                .expect("image_public_id must exist when image_changed is true"),
            admin_user_id,
            req.image_url
                .as_deref()
                .expect("image_url must be present when image_public_id is present"),
            config.product_image_upload_ttl_minutes,
        )
        .await?;
    }

    let product = sqlx::query_as::<_, Product>(
        r#"UPDATE products SET
            name = COALESCE($1, name),
            description = COALESCE($2, description),
            image_url = COALESCE($3, image_url),
            image_public_id = COALESCE($4, image_public_id),
            current_price = COALESCE($5, current_price),
            stock = COALESCE($6, stock),
            is_active = COALESCE($7, is_active),
            updated_at = $8
           WHERE id = $9
           RETURNING id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at"#,
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.image_url)
    .bind(&req.image_public_id)
    .bind(req.current_price)
    .bind(req.stock)
    .bind(req.is_active)
    .bind(Utc::now())
    .bind(product_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Product not found".to_string()))?;

    tx.commit().await?;

    Ok(product)
}

/// Delete a product (soft delete by setting is_active = false)
pub async fn delete_product(pool: &PgPool, product_id: Uuid) -> Result<(), AppError> {
    let result =
        sqlx::query("UPDATE products SET is_active = FALSE, updated_at = $1 WHERE id = $2")
            .bind(Utc::now())
            .bind(product_id)
            .execute(pool)
            .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Product not found".to_string()));
    }

    Ok(())
}

pub async fn save_pending_product_image(
    pool: &PgPool,
    public_id: &str,
    secure_url: &str,
    uploaded_by: Uuid,
    ttl_minutes: i64,
) -> Result<(), AppError> {
    let expires_at = Utc::now() + Duration::minutes(ttl_minutes);

    sqlx::query(
        r#"INSERT INTO pending_product_images (public_id, secure_url, uploaded_by, expires_at)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (public_id) DO UPDATE
           SET secure_url = EXCLUDED.secure_url,
               uploaded_by = EXCLUDED.uploaded_by,
               expires_at = EXCLUDED.expires_at"#,
    )
    .bind(public_id)
    .bind(secure_url)
    .bind(uploaded_by)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn enqueue_pending_product_image_deletion(
    pool: &PgPool,
    public_id: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO pending_product_image_deletions (public_id)
           VALUES ($1)
           ON CONFLICT (public_id) DO NOTHING"#,
    )
    .bind(public_id)
    .execute(pool)
    .await?;

    Ok(())
}

async fn consume_pending_product_image(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    public_id: &str,
    uploaded_by: Uuid,
    expected_secure_url: &str,
    ttl_minutes: i64,
) -> Result<(), AppError> {
    let pending = sqlx::query_as::<_, (String,)>(
        r#"DELETE FROM pending_product_images
           WHERE public_id = $1
             AND uploaded_by = $2
             AND expires_at > NOW()
           RETURNING secure_url"#,
    )
    .bind(public_id)
    .bind(uploaded_by)
    .fetch_optional(&mut **tx)
    .await?;

    let (stored_secure_url,) = pending.ok_or_else(|| {
        AppError::BadRequest(format!(
            "Uploaded image is invalid, expired, or not owned by this admin. Please upload again. Images expire after {} minutes.",
            ttl_minutes
        ))
    })?;

    if stored_secure_url != expected_secure_url {
        return Err(AppError::BadRequest(
            "image_url does not match the uploaded image".to_string(),
        ));
    }

    Ok(())
}
