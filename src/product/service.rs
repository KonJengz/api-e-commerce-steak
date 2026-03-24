use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::shared::errors::AppError;

use super::model::{CreateProductRequest, Product, UpdateProductRequest};

/// List all active products
pub async fn list_products(pool: &PgPool) -> Result<Vec<Product>, AppError> {
    let products = sqlx::query_as::<_, Product>(
        r#"SELECT id, name, description, image_url, current_price, stock, is_active, created_at, updated_at
           FROM products WHERE is_active = TRUE
           ORDER BY created_at DESC"#,
    )
    .fetch_all(pool)
    .await?;

    Ok(products)
}

/// Get a product by ID
pub async fn get_product(pool: &PgPool, product_id: Uuid) -> Result<Product, AppError> {
    let product = sqlx::query_as::<_, Product>(
        r#"SELECT id, name, description, image_url, current_price, stock, is_active, created_at, updated_at
           FROM products WHERE id = $1 AND is_active = TRUE"#,
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
) -> Result<Product, AppError> {
    let id = Uuid::now_v7();
    let now = Utc::now();
    let stock = req.stock.unwrap_or(0);

    let product = sqlx::query_as::<_, Product>(
        r#"INSERT INTO products (id, name, description, image_url, current_price, stock, is_active, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, TRUE, $7, $7)
           RETURNING id, name, description, image_url, current_price, stock, is_active, created_at, updated_at"#,
    )
    .bind(id)
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.image_url)
    .bind(req.current_price)
    .bind(stock)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(product)
}

/// Update a product (admin only)
pub async fn update_product(
    pool: &PgPool,
    product_id: Uuid,
    req: &UpdateProductRequest,
) -> Result<Product, AppError> {
    let product = sqlx::query_as::<_, Product>(
        r#"UPDATE products SET
            name = COALESCE($1, name),
            description = COALESCE($2, description),
            image_url = COALESCE($3, image_url),
            current_price = COALESCE($4, current_price),
            stock = COALESCE($5, stock),
            is_active = COALESCE($6, is_active),
            updated_at = $7
           WHERE id = $8
           RETURNING id, name, description, image_url, current_price, stock, is_active, created_at, updated_at"#,
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.image_url)
    .bind(req.current_price)
    .bind(req.stock)
    .bind(req.is_active)
    .bind(Utc::now())
    .bind(product_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Product not found".to_string()))?;

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
