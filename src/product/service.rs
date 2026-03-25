use chrono::{Duration, Utc};
use sqlx::{Executor, PgPool, Postgres, Transaction};
use std::collections::HashSet;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::shared::errors::AppError;
use crate::shared::pagination::{PaginatedResponse, PaginationQuery};

use super::model::{
    AttachProductImageRequest, CreateProductRequest, Product, ProductImage,
    ProductImageMutationResponse, ReorderProductImagesRequest, UpdateProductRequest,
};

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

pub async fn list_product_images(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<Vec<ProductImage>, AppError> {
    fetch_product_images(pool, product_id).await
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
    let requested_image = match (req.image_url.as_deref(), req.image_public_id.as_deref()) {
        (Some(image_url), Some(image_public_id)) => {
            consume_pending_product_image(
                &mut tx,
                image_public_id,
                admin_user_id,
                image_url,
                config.product_image_upload_ttl_minutes,
            )
            .await?;

            Some((image_url, image_public_id))
        }
        _ => None,
    };

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

    if let Some((image_url, image_public_id)) = requested_image {
        sqlx::query(
            r#"INSERT INTO product_images (id, product_id, image_url, image_public_id, sort_order, is_primary, created_at)
               VALUES ($1, $2, $3, $4, 0, TRUE, $5)"#,
        )
        .bind(Uuid::now_v7())
        .bind(id)
        .bind(image_url)
        .bind(image_public_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

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
    let existing_product = lock_product_for_update(&mut tx, product_id).await?;
    let incoming_image_public_id = req.image_public_id.as_deref();
    let incoming_image_url = req.image_url.as_deref();
    let image_changed = incoming_image_public_id.is_some()
        && incoming_image_public_id != existing_product.image_public_id.as_deref();

    if incoming_image_public_id == existing_product.image_public_id.as_deref()
        && incoming_image_url.is_some()
        && incoming_image_url != existing_product.image_url.as_deref()
    {
        return Err(AppError::BadRequest(
            "image_url does not match the current primary image".to_string(),
        ));
    }

    if image_changed {
        consume_pending_product_image(
            &mut tx,
            incoming_image_public_id
                .expect("image_public_id must exist when image_changed is true"),
            admin_user_id,
            incoming_image_url.expect("image_url must be present when image_public_id is present"),
            config.product_image_upload_ttl_minutes,
        )
        .await?;
    }

    sqlx::query(
        r#"UPDATE products SET
            name = COALESCE($1, name),
            description = COALESCE($2, description),
            current_price = COALESCE($3, current_price),
            stock = COALESCE($4, stock),
            is_active = COALESCE($5, is_active),
            updated_at = $6
           WHERE id = $7"#,
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(req.current_price)
    .bind(req.stock)
    .bind(req.is_active)
    .bind(Utc::now())
    .bind(product_id)
    .execute(&mut *tx)
    .await?;

    if image_changed {
        replace_primary_product_image(
            &mut tx,
            product_id,
            existing_product.image_public_id.as_deref(),
            incoming_image_url.expect("image_url must exist when image_changed is true"),
            incoming_image_public_id
                .expect("image_public_id must exist when image_changed is true"),
        )
        .await?;
    }

    let product = fetch_product_for_admin(&mut *tx, product_id).await?;

    tx.commit().await?;

    Ok(product)
}

pub async fn attach_product_image(
    pool: &PgPool,
    product_id: Uuid,
    req: &AttachProductImageRequest,
    admin_user_id: Uuid,
    config: &AppConfig,
) -> Result<ProductImageMutationResponse, AppError> {
    let mut tx = pool.begin().await?;
    lock_product_for_update(&mut tx, product_id).await?;

    consume_pending_product_image(
        &mut tx,
        &req.image_public_id,
        admin_user_id,
        &req.image_url,
        config.product_image_upload_ttl_minutes,
    )
    .await?;

    let current_images = fetch_product_images(&mut *tx, product_id).await?;
    let new_image_id = Uuid::now_v7();
    let should_be_primary = req.is_primary.unwrap_or(current_images.is_empty());

    sqlx::query(
        r#"INSERT INTO product_images (id, product_id, image_url, image_public_id, sort_order, is_primary, created_at)
           VALUES ($1, $2, $3, $4, $5, FALSE, $6)"#,
    )
    .bind(new_image_id)
    .bind(product_id)
    .bind(&req.image_url)
    .bind(&req.image_public_id)
    .bind(current_images.len() as i32)
    .bind(Utc::now())
    .execute(&mut *tx)
    .await?;

    let primary_image_id = if should_be_primary {
        Some(new_image_id)
    } else {
        current_images
            .iter()
            .find(|image| image.is_primary)
            .map(|image| image.id)
            .or(Some(new_image_id))
    };

    let mut ordered_ids = current_images
        .iter()
        .map(|image| image.id)
        .collect::<Vec<_>>();
    if should_be_primary {
        ordered_ids.insert(0, new_image_id);
    } else {
        ordered_ids.push(new_image_id);
    }

    apply_product_image_order(&mut tx, product_id, &ordered_ids, primary_image_id).await?;
    let product = sync_product_primary_image(&mut tx, product_id).await?;
    let images = fetch_product_images(&mut *tx, product_id).await?;

    tx.commit().await?;

    Ok(ProductImageMutationResponse { product, images })
}

pub async fn clear_primary_product_image(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<(ProductImageMutationResponse, Option<String>), AppError> {
    delete_product_image_internal(pool, product_id, None).await
}

pub async fn delete_product_image(
    pool: &PgPool,
    product_id: Uuid,
    image_id: Uuid,
) -> Result<(ProductImageMutationResponse, Option<String>), AppError> {
    delete_product_image_internal(pool, product_id, Some(image_id)).await
}

pub async fn reorder_product_images(
    pool: &PgPool,
    product_id: Uuid,
    req: &ReorderProductImagesRequest,
) -> Result<ProductImageMutationResponse, AppError> {
    let mut tx = pool.begin().await?;
    lock_product_for_update(&mut tx, product_id).await?;

    let current_images = fetch_product_images(&mut *tx, product_id).await?;
    if current_images.is_empty() {
        return Err(AppError::NotFound(
            "Product does not have any images to reorder".to_string(),
        ));
    }

    let current_ids = current_images
        .iter()
        .map(|image| image.id)
        .collect::<HashSet<_>>();
    let requested_ids = req.image_ids.iter().copied().collect::<HashSet<_>>();

    if current_ids.len() != requested_ids.len()
        || current_ids.len() != current_images.len()
        || req.image_ids.len() != current_images.len()
        || !current_ids
            .iter()
            .all(|image_id| requested_ids.contains(image_id))
    {
        return Err(AppError::BadRequest(
            "image_ids must contain each current product image exactly once".to_string(),
        ));
    }

    let primary_image_id = req.image_ids.first().copied();
    apply_product_image_order(&mut tx, product_id, &req.image_ids, primary_image_id).await?;
    let product = sync_product_primary_image(&mut tx, product_id).await?;
    let images = fetch_product_images(&mut *tx, product_id).await?;

    tx.commit().await?;

    Ok(ProductImageMutationResponse { product, images })
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

async fn fetch_product_images<'e, E>(
    executor: E,
    product_id: Uuid,
) -> Result<Vec<ProductImage>, AppError>
where
    E: Executor<'e, Database = Postgres>,
{
    let images = sqlx::query_as::<_, ProductImage>(
        r#"SELECT id, product_id, image_url, image_public_id, sort_order, is_primary, created_at
           FROM product_images
           WHERE product_id = $1
           ORDER BY is_primary DESC, sort_order ASC, created_at ASC"#,
    )
    .bind(product_id)
    .fetch_all(executor)
    .await?;

    Ok(images)
}

async fn fetch_product_for_admin<'e, E>(executor: E, product_id: Uuid) -> Result<Product, AppError>
where
    E: Executor<'e, Database = Postgres>,
{
    let product = sqlx::query_as::<_, Product>(
        r#"SELECT id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at
           FROM products
           WHERE id = $1"#,
    )
    .bind(product_id)
    .fetch_optional(executor)
    .await?
    .ok_or_else(|| AppError::NotFound("Product not found".to_string()))?;

    Ok(product)
}

async fn lock_product_for_update(
    tx: &mut Transaction<'_, Postgres>,
    product_id: Uuid,
) -> Result<Product, AppError> {
    let product = sqlx::query_as::<_, Product>(
        r#"SELECT id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at
           FROM products
           WHERE id = $1
           FOR UPDATE"#,
    )
    .bind(product_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Product not found".to_string()))?;

    Ok(product)
}

async fn replace_primary_product_image(
    tx: &mut Transaction<'_, Postgres>,
    product_id: Uuid,
    old_public_id: Option<&str>,
    new_image_url: &str,
    new_image_public_id: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE product_images SET is_primary = FALSE WHERE product_id = $1")
        .bind(product_id)
        .execute(&mut **tx)
        .await?;

    if let Some(old_public_id) = old_public_id {
        sqlx::query("DELETE FROM product_images WHERE product_id = $1 AND image_public_id = $2")
            .bind(product_id)
            .bind(old_public_id)
            .execute(&mut **tx)
            .await?;
    }

    sqlx::query("UPDATE product_images SET sort_order = sort_order + 1 WHERE product_id = $1")
        .bind(product_id)
        .execute(&mut **tx)
        .await?;

    sqlx::query(
        r#"INSERT INTO product_images (id, product_id, image_url, image_public_id, sort_order, is_primary, created_at)
           VALUES ($1, $2, $3, $4, 0, TRUE, $5)"#,
    )
    .bind(Uuid::now_v7())
    .bind(product_id)
    .bind(new_image_url)
    .bind(new_image_public_id)
    .bind(Utc::now())
    .execute(&mut **tx)
    .await?;

    let _ = sync_product_primary_image(tx, product_id).await?;

    Ok(())
}

async fn apply_product_image_order(
    tx: &mut Transaction<'_, Postgres>,
    product_id: Uuid,
    ordered_ids: &[Uuid],
    primary_image_id: Option<Uuid>,
) -> Result<(), AppError> {
    let effective_primary_image_id = primary_image_id.or_else(|| ordered_ids.first().copied());

    for (index, image_id) in ordered_ids.iter().enumerate() {
        let result = sqlx::query(
            r#"UPDATE product_images
               SET sort_order = $1,
                   is_primary = $2
               WHERE id = $3
                 AND product_id = $4"#,
        )
        .bind(index as i32)
        .bind(Some(*image_id) == effective_primary_image_id)
        .bind(image_id)
        .bind(product_id)
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::BadRequest(
                "One or more product images are invalid for this product".to_string(),
            ));
        }
    }

    Ok(())
}

async fn sync_product_primary_image(
    tx: &mut Transaction<'_, Postgres>,
    product_id: Uuid,
) -> Result<Product, AppError> {
    let primary_image = sqlx::query_as::<_, ProductImage>(
        r#"SELECT id, product_id, image_url, image_public_id, sort_order, is_primary, created_at
           FROM product_images
           WHERE product_id = $1
           ORDER BY is_primary DESC, sort_order ASC, created_at ASC
           LIMIT 1"#,
    )
    .bind(product_id)
    .fetch_optional(&mut **tx)
    .await?;

    let product = sqlx::query_as::<_, Product>(
        r#"UPDATE products
           SET image_url = $1,
               image_public_id = $2,
               updated_at = $3
           WHERE id = $4
           RETURNING id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at"#,
    )
    .bind(primary_image.as_ref().map(|image| image.image_url.as_str()))
    .bind(primary_image.as_ref().map(|image| image.image_public_id.as_str()))
    .bind(Utc::now())
    .bind(product_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Product not found".to_string()))?;

    Ok(product)
}

async fn delete_product_image_internal(
    pool: &PgPool,
    product_id: Uuid,
    image_id: Option<Uuid>,
) -> Result<(ProductImageMutationResponse, Option<String>), AppError> {
    let mut tx = pool.begin().await?;
    let existing_product = lock_product_for_update(&mut tx, product_id).await?;
    let current_images = fetch_product_images(&mut *tx, product_id).await?;

    if current_images.is_empty() {
        if image_id.is_none()
            && (existing_product.image_url.is_some() || existing_product.image_public_id.is_some())
        {
            let product = sqlx::query_as::<_, Product>(
                r#"UPDATE products
                   SET image_url = NULL,
                       image_public_id = NULL,
                       updated_at = $1
                   WHERE id = $2
                   RETURNING id, name, description, image_url, image_public_id, current_price, stock, is_active, created_at, updated_at"#,
            )
            .bind(Utc::now())
            .bind(product_id)
            .fetch_one(&mut *tx)
            .await?;

            tx.commit().await?;

            return Ok((
                ProductImageMutationResponse {
                    product,
                    images: Vec::new(),
                },
                existing_product.image_public_id,
            ));
        }

        return Err(AppError::NotFound(
            "Product does not have any images".to_string(),
        ));
    }

    let target_image = if let Some(image_id) = image_id {
        current_images
            .iter()
            .find(|image| image.id == image_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound("Product image not found".to_string()))?
    } else {
        current_images
            .iter()
            .find(|image| image.is_primary)
            .cloned()
            .or_else(|| current_images.first().cloned())
            .ok_or_else(|| {
                AppError::NotFound("Product does not have a primary image".to_string())
            })?
    };

    sqlx::query("DELETE FROM product_images WHERE id = $1 AND product_id = $2")
        .bind(target_image.id)
        .bind(product_id)
        .execute(&mut *tx)
        .await?;

    let remaining_images = current_images
        .into_iter()
        .filter(|image| image.id != target_image.id)
        .collect::<Vec<_>>();
    let ordered_ids = remaining_images
        .iter()
        .map(|image| image.id)
        .collect::<Vec<_>>();
    let primary_image_id = if remaining_images.is_empty() {
        None
    } else if target_image.is_primary {
        Some(remaining_images[0].id)
    } else {
        remaining_images
            .iter()
            .find(|image| image.is_primary)
            .map(|image| image.id)
            .or_else(|| ordered_ids.first().copied())
    };

    apply_product_image_order(&mut tx, product_id, &ordered_ids, primary_image_id).await?;
    let product = sync_product_primary_image(&mut tx, product_id).await?;
    let images = fetch_product_images(&mut *tx, product_id).await?;

    tx.commit().await?;

    Ok((
        ProductImageMutationResponse { product, images },
        Some(target_image.image_public_id),
    ))
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
