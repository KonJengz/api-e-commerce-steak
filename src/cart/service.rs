use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::shared::errors::AppError;

use super::model::*;

#[derive(sqlx::FromRow)]
pub struct CartRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ProductAvailability {
    stock: i32,
    is_active: bool,
}

/// Get existing cart or create a new one for the user
pub async fn get_or_create_cart(pool: &PgPool, user_id: Uuid) -> Result<CartRecord, AppError> {
    let cart = sqlx::query_as::<_, CartRecord>(
        r#"INSERT INTO carts (id, user_id, created_at, updated_at)
           VALUES ($1, $2, NOW(), NOW())
           ON CONFLICT (user_id)
           DO UPDATE SET updated_at = carts.updated_at
           RETURNING id, user_id, created_at, updated_at"#,
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(cart)
}

/// Fetches all items in a specific cart, joined with product details
pub async fn get_cart_items(pool: &PgPool, cart_id: Uuid) -> Result<Vec<CartItem>, AppError> {
    let items = sqlx::query_as::<_, CartItem>(
        r#"SELECT 
               ci.id, 
               ci.product_id, 
               p.name as product_name, 
               p.image_url as product_image_url, 
               p.current_price, 
               p.stock, 
               p.is_active, 
               ci.quantity, 
               ci.created_at, 
               ci.updated_at
           FROM cart_items ci
           JOIN products p ON p.id = ci.product_id
           WHERE ci.cart_id = $1
           ORDER BY ci.created_at ASC"#,
    )
    .bind(cart_id)
    .fetch_all(pool)
    .await?;

    Ok(items)
}

/// Add an item to the cart (or increment quantity if it exists)
pub async fn add_item_to_cart(
    pool: &PgPool,
    cart_id: Uuid,
    req: &AddCartItemRequest,
) -> Result<(), AppError> {
    let product = fetch_active_product_availability(pool, req.product_id).await?;
    if product.stock <= 0 {
        return Err(AppError::BadRequest("Product is out of stock".to_string()));
    }

    if req.quantity > product.stock {
        return Err(AppError::BadRequest(
            "Requested quantity exceeds available stock".to_string(),
        ));
    }

    let item_id = Uuid::now_v7();
    let result = sqlx::query(
        r#"INSERT INTO cart_items (id, cart_id, product_id, quantity, created_at, updated_at)
           VALUES ($1, $2, $3, $4, NOW(), NOW())
           ON CONFLICT (cart_id, product_id)
           DO UPDATE SET 
               quantity = cart_items.quantity + EXCLUDED.quantity,
               updated_at = NOW()
           WHERE cart_items.quantity + EXCLUDED.quantity <= $5"#,
    )
    .bind(item_id)
    .bind(cart_id)
    .bind(req.product_id)
    .bind(req.quantity)
    .bind(product.stock)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::BadRequest(
            "Requested quantity exceeds available stock".to_string(),
        ));
    }

    Ok(())
}

/// Replace the exact quantity of an existing cart item
pub async fn update_cart_item_quantity(
    pool: &PgPool,
    cart_id: Uuid,
    product_id: Uuid,
    quantity: i32,
) -> Result<(), AppError> {
    let product = fetch_active_product_availability(pool, product_id).await?;
    if product.stock <= 0 {
        return Err(AppError::BadRequest("Product is out of stock".to_string()));
    }

    if quantity > product.stock {
        return Err(AppError::BadRequest(
            "Requested quantity exceeds available stock".to_string(),
        ));
    }

    let result = sqlx::query(
        r#"UPDATE cart_items 
           SET quantity = $1, updated_at = NOW()
           WHERE cart_id = $2 AND product_id = $3"#,
    )
    .bind(quantity)
    .bind(cart_id)
    .bind(product_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Item not found in cart".to_string()));
    }

    Ok(())
}

async fn fetch_active_product_availability(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<ProductAvailability, AppError> {
    let product = sqlx::query_as::<_, ProductAvailability>(
        "SELECT stock, is_active FROM products WHERE id = $1",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Product not found".to_string()))?;

    if !product.is_active {
        return Err(AppError::NotFound(
            "Product not found or inactive".to_string(),
        ));
    }

    Ok(product)
}

/// Remove a specific product from the cart
pub async fn remove_item_from_cart(
    pool: &PgPool,
    cart_id: Uuid,
    product_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM cart_items WHERE cart_id = $1 AND product_id = $2")
        .bind(cart_id)
        .bind(product_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Empty the entire cart
pub async fn clear_cart(pool: &PgPool, cart_id: Uuid) -> Result<(), AppError> {
    sqlx::query("DELETE FROM cart_items WHERE cart_id = $1")
        .bind(cart_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Internal transactional version: clear cart by user_id
pub async fn clear_cart_by_user_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<(), AppError> {
    let cart_id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM carts WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?;

    if let Some(id) = cart_id {
        sqlx::query("DELETE FROM cart_items WHERE cart_id = $1")
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}
