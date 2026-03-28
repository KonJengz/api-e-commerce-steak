use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::shared::errors::AppError;
use crate::shared::pagination::{PaginatedResponse, PaginationQuery};

use super::model::*;

const ORDER_STATUS_PENDING: &str = "PENDING";
const ORDER_STATUS_PAID: &str = "PAID";
const ORDER_STATUS_SHIPPED: &str = "SHIPPED";
const ORDER_STATUS_DELIVERED: &str = "DELIVERED";
const ORDER_STATUS_CANCELLED: &str = "CANCELLED";

pub struct TrackingEmailNotification {
    pub user_email: String,
    pub order_id: String,
    pub tracking_number: String,
}

/// Create a new order with price snapshots
pub async fn create_order(
    pool: &PgPool,
    user_id: Uuid,
    req: &CreateOrderRequest,
) -> Result<OrderResponse, AppError> {
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

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to begin transaction: {}", e)))?;

    let order_id = Uuid::now_v7();
    let now = Utc::now();
    let mut total_amount = Decimal::ZERO;
    let mut order_items: Vec<OrderItem> = Vec::new();

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

    sqlx::query(
        r#"INSERT INTO orders (id, user_id, shipping_address_id, total_amount, status, tracking_number, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, NULL, $6, $6)"#,
    )
    .bind(order_id)
    .bind(user_id)
    .bind(req.shipping_address_id)
    .bind(total_amount)
    .bind(ORDER_STATUS_PENDING)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    crate::cart::service::clear_cart_by_user_id_tx(&mut tx, user_id).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to commit transaction: {}", e)))?;

    Ok(OrderResponse {
        id: order_id,
        user_id,
        shipping_address_id: Some(req.shipping_address_id),
        total_amount,
        status: ORDER_STATUS_PENDING.to_string(),
        tracking_number: None,
        created_at: now,
        updated_at: now,
        items: order_items,
    })
}

/// List orders for a user
pub async fn list_orders(
    pool: &PgPool,
    user_id: Uuid,
    query: PaginationQuery,
) -> Result<PaginatedResponse<Order>, AppError> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await?;

    let orders = sqlx::query_as::<_, Order>(
        r#"SELECT id, user_id, shipping_address_id, total_amount, status, tracking_number, created_at, updated_at
           FROM orders WHERE user_id = $1
           ORDER BY created_at DESC
           LIMIT $2 OFFSET $3"#,
    )
    .bind(user_id)
    .bind(query.limit())
    .bind(query.offset())
    .fetch_all(pool)
    .await?;

    Ok(PaginatedResponse::new(
        orders,
        total,
        query.page(),
        query.limit(),
    ))
}

/// Get an order with its items (verifies ownership)
pub async fn get_order(
    pool: &PgPool,
    user_id: Uuid,
    order_id: Uuid,
) -> Result<OrderResponse, AppError> {
    let order = sqlx::query_as::<_, Order>(
        r#"SELECT id, user_id, shipping_address_id, total_amount, status, tracking_number, created_at, updated_at
           FROM orders WHERE id = $1 AND user_id = $2"#,
    )
    .bind(order_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Order not found".to_string()))?;

    let items = fetch_order_items(pool, order_id).await?;
    Ok(order_to_response(order, items))
}

/// List all orders for admin
pub async fn list_orders_for_admin(
    pool: &PgPool,
    query: AdminOrderListQuery,
) -> Result<PaginatedResponse<AdminOrder>, AppError> {
    let normalized_status = normalize_optional_order_status(query.status.as_deref())?;
    let normalized_search = normalize_admin_order_search(query.search.as_deref())?;
    let search_order_id = normalized_search
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok());

    let mut count_query = QueryBuilder::new(
        "SELECT COUNT(*) FROM orders o INNER JOIN users u ON u.id = o.user_id WHERE 1 = 1",
    );
    let mut data_query = QueryBuilder::new(
        r#"SELECT
               o.id,
               o.user_id,
               u.name AS user_name,
               u.email AS user_email,
               o.shipping_address_id,
               o.total_amount,
               o.status,
               o.tracking_number,
               o.created_at,
               o.updated_at
           FROM orders o
           INNER JOIN users u ON u.id = o.user_id
           WHERE 1 = 1"#,
    );

    if let Some(status) = normalized_status.as_deref() {
        count_query.push(" AND o.status = ");
        count_query.push_bind(status);

        data_query.push(" AND o.status = ");
        data_query.push_bind(status);
    }

    if let Some(search) = normalized_search.as_deref() {
        let prefix = format!("{}%", search.to_ascii_lowercase());
        push_admin_order_search_clause(&mut count_query, &prefix, search_order_id);
        push_admin_order_search_clause(&mut data_query, &prefix, search_order_id);
    }

    data_query.push(" ORDER BY o.created_at DESC ");
    data_query.push(" LIMIT ");
    data_query.push_bind(query.limit());
    data_query.push(" OFFSET ");
    data_query.push_bind(query.offset());

    let total: i64 = count_query.build_query_scalar().fetch_one(pool).await?;
    let orders = data_query
        .build_query_as::<AdminOrder>()
        .fetch_all(pool)
        .await?;

    Ok(PaginatedResponse::new(
        orders,
        total,
        query.page(),
        query.limit(),
    ))
}

/// Get a single order with items for admin
pub async fn get_order_for_admin(
    pool: &PgPool,
    order_id: Uuid,
) -> Result<AdminOrderResponse, AppError> {
    let order = fetch_admin_order(pool, order_id).await?;
    let items = fetch_order_items(pool, order_id).await?;
    Ok(admin_order_to_response(order, items))
}

/// Update an order as admin, including status and optional tracking number.
pub async fn update_order_for_admin(
    pool: &PgPool,
    order_id: Uuid,
    req: &UpdateOrderRequest,
) -> Result<(AdminOrderResponse, Option<TrackingEmailNotification>), AppError> {
    let normalized_status = normalize_order_status(&req.status)?;
    let normalized_tracking = normalize_tracking_number(req.tracking_number.as_deref())?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to begin transaction: {}", e)))?;

    let current_order = sqlx::query_as::<_, (Uuid, String, Option<String>, String, String)>(
        r#"SELECT o.user_id, o.status, o.tracking_number, u.name, u.email
           FROM orders o
           INNER JOIN users u ON u.id = o.user_id
           WHERE o.id = $1
           FOR UPDATE"#,
    )
    .bind(order_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Order not found".to_string()))?;

    let (user_id, current_status, current_tracking_number, user_name, user_email) = current_order;

    validate_order_status_transition(&current_status, &normalized_status)?;

    let final_tracking_number = normalized_tracking
        .clone()
        .or_else(|| current_tracking_number.clone());

    if final_tracking_number.is_some() && !status_supports_tracking(&normalized_status) {
        return Err(AppError::BadRequest(
            "Tracking number can only be set when the order status is SHIPPED or DELIVERED"
                .to_string(),
        ));
    }

    if normalized_status == ORDER_STATUS_SHIPPED && final_tracking_number.is_none() {
        return Err(AppError::BadRequest(
            "Tracking number is required when marking an order as SHIPPED".to_string(),
        ));
    }

    if should_restore_stock(&current_status, &normalized_status) {
        sqlx::query(
            r#"UPDATE products AS p
               SET stock = p.stock + oi.quantity,
                   updated_at = NOW()
               FROM order_items AS oi
               WHERE oi.order_id = $1
                 AND oi.product_id = p.id"#,
        )
        .bind(order_id)
        .execute(&mut *tx)
        .await?;
    }

    let updated_order = sqlx::query_as::<_, Order>(
        r#"UPDATE orders
           SET status = $1,
               tracking_number = $2,
               updated_at = NOW()
           WHERE id = $3
           RETURNING id, user_id, shipping_address_id, total_amount, status, tracking_number, created_at, updated_at"#,
    )
    .bind(&normalized_status)
    .bind(final_tracking_number.as_deref())
    .bind(order_id)
    .fetch_one(&mut *tx)
    .await?;

    let items = sqlx::query_as::<_, OrderItem>(
        r#"SELECT id, order_id, product_id, product_name_at_purchase, quantity, price_at_purchase
           FROM order_items WHERE order_id = $1
           ORDER BY id ASC"#,
    )
    .bind(order_id)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to commit transaction: {}", e)))?;

    let notification = match (
        current_tracking_number.as_deref(),
        updated_order.tracking_number.as_deref(),
    ) {
        (_, None) => None,
        (Some(previous), Some(current)) if previous == current => None,
        (_, Some(current)) => Some(TrackingEmailNotification {
            user_email: user_email.clone(),
            order_id: order_id.to_string(),
            tracking_number: current.to_string(),
        }),
    };

    let order = AdminOrderResponse {
        id: updated_order.id,
        user_id,
        user_name,
        user_email,
        shipping_address_id: updated_order.shipping_address_id,
        total_amount: updated_order.total_amount,
        status: updated_order.status,
        tracking_number: updated_order.tracking_number,
        created_at: updated_order.created_at,
        updated_at: updated_order.updated_at,
        items,
    };

    Ok((order, notification))
}

async fn fetch_admin_order(pool: &PgPool, order_id: Uuid) -> Result<AdminOrder, AppError> {
    sqlx::query_as::<_, AdminOrder>(
        r#"SELECT
               o.id,
               o.user_id,
               u.name AS user_name,
               u.email AS user_email,
               o.shipping_address_id,
               o.total_amount,
               o.status,
               o.tracking_number,
               o.created_at,
               o.updated_at
           FROM orders o
           INNER JOIN users u ON u.id = o.user_id
           WHERE o.id = $1"#,
    )
    .bind(order_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Order not found".to_string()))
}

async fn fetch_order_items(pool: &PgPool, order_id: Uuid) -> Result<Vec<OrderItem>, AppError> {
    sqlx::query_as::<_, OrderItem>(
        r#"SELECT id, order_id, product_id, product_name_at_purchase, quantity, price_at_purchase
           FROM order_items
           WHERE order_id = $1
           ORDER BY id ASC"#,
    )
    .bind(order_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::from)
}

fn order_to_response(order: Order, items: Vec<OrderItem>) -> OrderResponse {
    OrderResponse {
        id: order.id,
        user_id: order.user_id,
        shipping_address_id: order.shipping_address_id,
        total_amount: order.total_amount,
        status: order.status,
        tracking_number: order.tracking_number,
        created_at: order.created_at,
        updated_at: order.updated_at,
        items,
    }
}

fn admin_order_to_response(order: AdminOrder, items: Vec<OrderItem>) -> AdminOrderResponse {
    AdminOrderResponse {
        id: order.id,
        user_id: order.user_id,
        user_name: order.user_name,
        user_email: order.user_email,
        shipping_address_id: order.shipping_address_id,
        total_amount: order.total_amount,
        status: order.status,
        tracking_number: order.tracking_number,
        created_at: order.created_at,
        updated_at: order.updated_at,
        items,
    }
}

fn normalize_order_status(status: &str) -> Result<String, AppError> {
    let normalized = status.trim().to_ascii_uppercase();

    if normalized.is_empty() {
        return Err(AppError::BadRequest("Order status is required".to_string()));
    }

    if !matches!(
        normalized.as_str(),
        ORDER_STATUS_PENDING
            | ORDER_STATUS_PAID
            | ORDER_STATUS_SHIPPED
            | ORDER_STATUS_DELIVERED
            | ORDER_STATUS_CANCELLED
    ) {
        return Err(AppError::BadRequest(
            "Invalid order status. Allowed values: PENDING, PAID, SHIPPED, DELIVERED, CANCELLED"
                .to_string(),
        ));
    }

    Ok(normalized)
}

fn normalize_tracking_number(tracking_number: Option<&str>) -> Result<Option<String>, AppError> {
    match tracking_number {
        None => Ok(None),
        Some(value) => {
            let trimmed = value.trim();

            if trimmed.is_empty() {
                return Err(AppError::BadRequest(
                    "Tracking number cannot be blank".to_string(),
                ));
            }

            if trimmed.chars().count() > 100 {
                return Err(AppError::BadRequest(
                    "Tracking number must be at most 100 characters".to_string(),
                ));
            }

            Ok(Some(trimmed.to_string()))
        }
    }
}

fn normalize_optional_order_status(status: Option<&str>) -> Result<Option<String>, AppError> {
    match status {
        None => Ok(None),
        Some(value) => normalize_order_status(value).map(Some),
    }
}

fn normalize_admin_order_search(search: Option<&str>) -> Result<Option<String>, AppError> {
    match search {
        None => Ok(None),
        Some(value) => {
            let trimmed = value.trim();

            if trimmed.is_empty() {
                return Ok(None);
            }

            if trimmed.chars().count() > 100 {
                return Err(AppError::BadRequest(
                    "Search must be at most 100 characters".to_string(),
                ));
            }

            Ok(Some(trimmed.to_string()))
        }
    }
}

fn push_admin_order_search_clause(
    query: &mut QueryBuilder<'_, Postgres>,
    prefix: &str,
    search_order_id: Option<Uuid>,
) {
    query.push(" AND (");

    let mut has_previous_clause = false;

    if let Some(order_id) = search_order_id {
        query.push("o.id = ");
        query.push_bind(order_id);
        has_previous_clause = true;
    }

    if has_previous_clause {
        query.push(" OR ");
    }

    query.push("LOWER(u.name) LIKE ");
    query.push_bind(prefix.to_string());
    query.push(" OR LOWER(u.email) LIKE ");
    query.push_bind(prefix.to_string());
    query.push(" OR LOWER(COALESCE(o.tracking_number, '')) LIKE ");
    query.push_bind(prefix.to_string());
    query.push(")");
}

fn status_supports_tracking(status: &str) -> bool {
    matches!(status, ORDER_STATUS_SHIPPED | ORDER_STATUS_DELIVERED)
}

fn validate_order_status_transition(current: &str, next: &str) -> Result<(), AppError> {
    if current == next {
        return Ok(());
    }

    let is_allowed = match current {
        ORDER_STATUS_PENDING => matches!(next, ORDER_STATUS_PAID | ORDER_STATUS_CANCELLED),
        ORDER_STATUS_PAID => matches!(next, ORDER_STATUS_SHIPPED | ORDER_STATUS_CANCELLED),
        ORDER_STATUS_SHIPPED => next == ORDER_STATUS_DELIVERED,
        ORDER_STATUS_DELIVERED | ORDER_STATUS_CANCELLED => false,
        _ => false,
    };

    if is_allowed {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "Cannot change order status from {} to {}",
            current, next
        )))
    }
}

fn should_restore_stock(current: &str, next: &str) -> bool {
    current != ORDER_STATUS_CANCELLED && next == ORDER_STATUS_CANCELLED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_status_accepts_valid_values() {
        assert_eq!(
            normalize_order_status(" shipped ").expect("status should normalize"),
            ORDER_STATUS_SHIPPED
        );
        assert_eq!(
            normalize_order_status("paid").expect("status should normalize"),
            ORDER_STATUS_PAID
        );
    }

    #[test]
    fn normalize_status_rejects_invalid_values() {
        assert!(matches!(
            normalize_order_status("processing"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn transition_rules_block_invalid_jumps() {
        assert!(matches!(
            validate_order_status_transition(ORDER_STATUS_PENDING, ORDER_STATUS_SHIPPED),
            Err(AppError::BadRequest(_))
        ));
        assert!(validate_order_status_transition(ORDER_STATUS_PAID, ORDER_STATUS_SHIPPED).is_ok());
    }

    #[test]
    fn tracking_number_is_trimmed_and_rejects_blank_values() {
        assert_eq!(
            normalize_tracking_number(Some(" TH123456 ")).expect("tracking should normalize"),
            Some("TH123456".to_string())
        );
        assert!(matches!(
            normalize_tracking_number(Some("   ")),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn cancelled_orders_restore_stock_once_when_entering_final_state() {
        assert!(should_restore_stock(
            ORDER_STATUS_PENDING,
            ORDER_STATUS_CANCELLED
        ));
        assert!(should_restore_stock(
            ORDER_STATUS_PAID,
            ORDER_STATUS_CANCELLED
        ));
        assert!(!should_restore_stock(
            ORDER_STATUS_CANCELLED,
            ORDER_STATUS_CANCELLED
        ));
        assert!(!should_restore_stock(
            ORDER_STATUS_SHIPPED,
            ORDER_STATUS_DELIVERED
        ));
    }

    #[test]
    fn admin_order_search_trims_blank_values_to_none() {
        assert_eq!(
            normalize_admin_order_search(Some("   ")).expect("blank search should normalize"),
            None
        );
        assert_eq!(
            normalize_admin_order_search(Some(" Jane ")).expect("search should normalize"),
            Some("Jane".to_string())
        );
    }

    #[test]
    fn optional_status_filter_normalizes_values() {
        assert_eq!(
            normalize_optional_order_status(Some(" paid ")).expect("status should normalize"),
            Some(ORDER_STATUS_PAID.to_string())
        );
        assert!(matches!(
            normalize_optional_order_status(Some("processing")),
            Err(AppError::BadRequest(_))
        ));
    }
}
