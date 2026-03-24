use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::shared::errors::AppError;

use super::model::*;

/// Create a new order with price snapshots
pub async fn create_order(
    pool: &PgPool,
    user_id: Uuid,
    req: &CreateOrderRequest,
) -> Result<OrderResponse, AppError> {
    // Verify the address belongs to the user
    let address_exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM addresses WHERE id = $1 AND user_id = $2",
    )
    .bind(req.shipping_address_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    if address_exists == 0 {
        return Err(AppError::BadRequest("Invalid shipping address".to_string()));
    }

    // Begin transaction
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to begin transaction: {}", e)))?;

    let order_id = Uuid::now_v7();
    let mut total_amount = Decimal::ZERO;
    let mut order_items: Vec<OrderItem> = Vec::new();

    // Process each item: snapshot price + name, check stock
    for item in &req.items {
        if item.quantity <= 0 {
            return Err(AppError::BadRequest(
                "Quantity must be positive".to_string(),
            ));
        }

        let product = sqlx::query_as::<_, (Uuid, String, Decimal, i32, bool)>(
            "SELECT id, name, current_price, stock, is_active FROM products WHERE id = $1 FOR UPDATE",
        )
        .bind(item.product_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Product {} not found", item.product_id)))?;

        let (product_id, product_name, current_price, stock, is_active) = product;

        if !is_active {
            return Err(AppError::BadRequest(format!(
                "Product '{}' is no longer available",
                product_name
            )));
        }

        if stock < item.quantity {
            return Err(AppError::BadRequest(format!(
                "Insufficient stock for '{}'. Available: {}, Requested: {}",
                product_name, stock, item.quantity
            )));
        }

        // Deduct stock under the same row lock, and keep the update guarded.
        let stock_update = sqlx::query(
            "UPDATE products SET stock = stock - $1, updated_at = NOW() WHERE id = $2 AND stock >= $1",
        )
            .bind(item.quantity)
            .bind(product_id)
            .execute(&mut *tx)
            .await?;

        if stock_update.rows_affected() == 0 {
            return Err(AppError::BadRequest(format!(
                "Insufficient stock for '{}'. Available: {}, Requested: {}",
                product_name, stock, item.quantity
            )));
        }

        // Snapshot price and name at purchase time
        let item_total = current_price * Decimal::from(item.quantity);
        total_amount += item_total;

        let order_item_id = Uuid::now_v7();
        sqlx::query(
            r#"INSERT INTO order_items (id, order_id, product_id, product_name_at_purchase, quantity, price_at_purchase)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(order_item_id)
        .bind(order_id)
        .bind(product_id)
        .bind(&product_name)
        .bind(item.quantity)
        .bind(current_price)
        .execute(&mut *tx)
        .await?;

        order_items.push(OrderItem {
            id: order_item_id,
            order_id,
            product_id,
            product_name_at_purchase: product_name,
            quantity: item.quantity,
            price_at_purchase: current_price,
        });
    }

    // Create order
    sqlx::query(
        r#"INSERT INTO orders (id, user_id, shipping_address_id, total_amount, status, created_at)
           VALUES ($1, $2, $3, $4, 'PENDING', NOW())"#,
    )
    .bind(order_id)
    .bind(user_id)
    .bind(req.shipping_address_id)
    .bind(total_amount)
    .execute(&mut *tx)
    .await?;

    // Commit transaction
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to commit transaction: {}", e)))?;

    Ok(OrderResponse {
        id: order_id,
        user_id,
        shipping_address_id: Some(req.shipping_address_id),
        total_amount,
        status: "PENDING".to_string(),
        created_at: chrono::Utc::now(),
        items: order_items,
    })
}

/// List orders for a user
pub async fn list_orders(pool: &PgPool, user_id: Uuid) -> Result<Vec<Order>, AppError> {
    let orders = sqlx::query_as::<_, Order>(
        r#"SELECT id, user_id, shipping_address_id, total_amount, status, created_at
           FROM orders WHERE user_id = $1
           ORDER BY created_at DESC"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(orders)
}

/// Get an order with its items (verifies ownership)
pub async fn get_order(
    pool: &PgPool,
    user_id: Uuid,
    order_id: Uuid,
) -> Result<OrderResponse, AppError> {
    let order = sqlx::query_as::<_, Order>(
        r#"SELECT id, user_id, shipping_address_id, total_amount, status, created_at
           FROM orders WHERE id = $1 AND user_id = $2"#,
    )
    .bind(order_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Order not found".to_string()))?;

    let items = sqlx::query_as::<_, OrderItem>(
        r#"SELECT id, order_id, product_id, product_name_at_purchase, quantity, price_at_purchase
           FROM order_items WHERE order_id = $1"#,
    )
    .bind(order_id)
    .fetch_all(pool)
    .await?;

    Ok(OrderResponse {
        id: order.id,
        user_id: order.user_id,
        shipping_address_id: order.shipping_address_id,
        total_amount: order.total_amount,
        status: order.status,
        created_at: order.created_at,
        items,
    })
}
