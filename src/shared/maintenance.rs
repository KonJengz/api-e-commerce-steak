use std::time::Duration;

use crate::config::AppConfig;
use crate::shared::cloudinary;
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use uuid::Uuid;

const UNVERIFIED_USER_RETENTION_DAYS: i64 = 7;
const STALE_ORDER_BATCH_SIZE: i64 = 100;
const ORDER_STATUS_PENDING: &str = "PENDING";
const ORDER_STATUS_PAYMENT_FAILED: &str = "PAYMENT_FAILED";
const ORDER_STATUS_CANCELLED: &str = "CANCELLED";

pub fn spawn_expired_data_cleanup(
    pool: PgPool,
    config: AppConfig,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;

            let cleanup_result = cleanup_expired_data(&pool, &config).await;

            match cleanup_result {
                Ok(stats) => {
                    if stats.deleted_refresh_tokens > 0
                        || stats.deleted_email_verifications > 0
                        || stats.deleted_oauth_login_tickets > 0
                        || stats.deleted_unverified_users > 0
                        || stats.cancelled_stale_orders > 0
                        || stats.deleted_pending_product_images > 0
                        || stats.deleted_pending_product_image_deletions > 0
                    {
                        tracing::info!(
                            deleted_refresh_tokens = stats.deleted_refresh_tokens,
                            deleted_email_verifications = stats.deleted_email_verifications,
                            deleted_oauth_login_tickets = stats.deleted_oauth_login_tickets,
                            deleted_unverified_users = stats.deleted_unverified_users,
                            cancelled_stale_orders = stats.cancelled_stale_orders,
                            deleted_pending_product_images = stats.deleted_pending_product_images,
                            deleted_pending_product_image_deletions =
                                stats.deleted_pending_product_image_deletions,
                            "expired records cleaned up"
                        );
                    }
                }
                Err(error) => {
                    tracing::error!(error = ?error, "failed to clean up expired records");
                }
            }
        }
    })
}

#[derive(Debug, Default)]
struct CleanupStats {
    deleted_refresh_tokens: u64,
    deleted_email_verifications: u64,
    deleted_oauth_login_tickets: u64,
    deleted_unverified_users: u64,
    cancelled_stale_orders: u64,
    deleted_pending_product_images: u64,
    deleted_pending_product_image_deletions: u64,
}

#[derive(sqlx::FromRow)]
struct ExpiredPendingProductImage {
    public_id: String,
}

#[derive(sqlx::FromRow)]
struct PendingProductImageDeletion {
    public_id: String,
}

#[derive(sqlx::FromRow)]
struct StaleOrderCandidate {
    id: Uuid,
}

async fn cleanup_expired_data(
    pool: &PgPool,
    config: &AppConfig,
) -> Result<CleanupStats, sqlx::Error> {
    let refresh_tokens = sqlx::query("DELETE FROM refresh_tokens WHERE expires_at <= NOW()")
        .execute(pool)
        .await?;

    let email_verifications =
        sqlx::query("DELETE FROM email_verifications WHERE expires_at <= NOW()")
            .execute(pool)
            .await?;

    let oauth_login_tickets =
        sqlx::query("DELETE FROM oauth_login_tickets WHERE expires_at <= NOW()")
            .execute(pool)
            .await?;

    let unverified_users = sqlx::query(
        r#"DELETE FROM users
           WHERE is_verified = FALSE
             AND updated_at <= NOW() - ($1 * INTERVAL '1 day')"#,
    )
    .bind(UNVERIFIED_USER_RETENTION_DAYS)
    .execute(pool)
    .await?;

    let cancelled_stale_orders = cancel_stale_orders(pool, config).await?;

    let expired_images = sqlx::query_as::<_, ExpiredPendingProductImage>(
        r#"SELECT public_id
           FROM pending_product_images
           WHERE expires_at <= NOW()
           ORDER BY expires_at ASC
           LIMIT 100"#,
    )
    .fetch_all(pool)
    .await?;

    let mut deleted_pending_product_images = 0;

    for image in expired_images {
        match cloudinary::delete_image(&image.public_id, config).await {
            Ok(()) => {
                let deleted =
                    sqlx::query("DELETE FROM pending_product_images WHERE public_id = $1")
                        .bind(&image.public_id)
                        .execute(pool)
                        .await?;
                deleted_pending_product_images += deleted.rows_affected();
            }
            Err(error) => {
                tracing::error!(
                    error = %error,
                    public_id = %image.public_id,
                    "failed to clean up expired pending product image"
                );
            }
        }
    }

    let pending_deletions = sqlx::query_as::<_, PendingProductImageDeletion>(
        r#"SELECT public_id
           FROM pending_product_image_deletions
           ORDER BY created_at ASC
           LIMIT 100"#,
    )
    .fetch_all(pool)
    .await?;

    let mut deleted_pending_product_image_deletions = 0;

    for image in pending_deletions {
        match cloudinary::delete_image(&image.public_id, config).await {
            Ok(()) => {
                let deleted =
                    sqlx::query("DELETE FROM pending_product_image_deletions WHERE public_id = $1")
                        .bind(&image.public_id)
                        .execute(pool)
                        .await?;
                deleted_pending_product_image_deletions += deleted.rows_affected();
            }
            Err(error) => {
                tracing::error!(
                    error = %error,
                    public_id = %image.public_id,
                    "failed to process queued product image deletion"
                );
            }
        }
    }

    Ok(CleanupStats {
        deleted_refresh_tokens: refresh_tokens.rows_affected(),
        deleted_email_verifications: email_verifications.rows_affected(),
        deleted_oauth_login_tickets: oauth_login_tickets.rows_affected(),
        deleted_unverified_users: unverified_users.rows_affected(),
        cancelled_stale_orders,
        deleted_pending_product_images,
        deleted_pending_product_image_deletions,
    })
}

async fn cancel_stale_orders(pool: &PgPool, config: &AppConfig) -> Result<u64, sqlx::Error> {
    if !stale_order_cleanup_enabled(config) {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    let stale_order_ids = fetch_stale_order_ids(&mut tx, config).await?;

    if stale_order_ids.is_empty() {
        tx.commit().await?;
        return Ok(0);
    }

    restore_stock_for_orders(&mut tx, &stale_order_ids).await?;

    let mut cancel_query = QueryBuilder::<Postgres>::new("UPDATE orders SET status = ");
    cancel_query.push_bind(ORDER_STATUS_CANCELLED);
    cancel_query.push(", updated_at = NOW() WHERE id IN (");

    {
        let mut separated = cancel_query.separated(", ");
        for order_id in &stale_order_ids {
            separated.push_bind(order_id);
        }
    }

    cancel_query.push(")");

    let cancelled_orders = cancel_query.build().execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(cancelled_orders.rows_affected())
}

fn stale_order_cleanup_enabled(config: &AppConfig) -> bool {
    config.order_pending_timeout_minutes > 0 || config.order_payment_failed_timeout_minutes > 0
}

async fn fetch_stale_order_ids(
    tx: &mut Transaction<'_, Postgres>,
    config: &AppConfig,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut query = QueryBuilder::<Postgres>::new("SELECT id FROM orders WHERE ");

    if !push_stale_order_conditions(&mut query, config) {
        return Ok(Vec::new());
    }

    query.push(" ORDER BY created_at ASC, id ASC FOR UPDATE SKIP LOCKED LIMIT ");
    query.push_bind(STALE_ORDER_BATCH_SIZE);

    let stale_orders = query
        .build_query_as::<StaleOrderCandidate>()
        .fetch_all(&mut **tx)
        .await?;

    Ok(stale_orders.into_iter().map(|order| order.id).collect())
}

fn push_stale_order_conditions(query: &mut QueryBuilder<'_, Postgres>, config: &AppConfig) -> bool {
    let mut has_condition = false;

    if config.order_pending_timeout_minutes > 0 {
        query.push("(");
        query.push("status = ");
        query.push_bind(ORDER_STATUS_PENDING);
        query.push(" AND created_at <= NOW() - (");
        query.push_bind(config.order_pending_timeout_minutes);
        query.push(" * INTERVAL '1 minute'))");
        has_condition = true;
    }

    if config.order_payment_failed_timeout_minutes > 0 {
        if has_condition {
            query.push(" OR ");
        }

        query.push("(");
        query.push("status = ");
        query.push_bind(ORDER_STATUS_PAYMENT_FAILED);
        query.push(" AND updated_at <= NOW() - (");
        query.push_bind(config.order_payment_failed_timeout_minutes);
        query.push(" * INTERVAL '1 minute'))");
        has_condition = true;
    }

    has_condition
}

async fn restore_stock_for_orders(
    tx: &mut Transaction<'_, Postgres>,
    order_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    let mut query = QueryBuilder::<Postgres>::new(
        r#"UPDATE products AS p
           SET stock = p.stock + restored.quantity,
               updated_at = NOW()
           FROM (
               SELECT oi.product_id, SUM(oi.quantity)::INT AS quantity
               FROM order_items AS oi
               WHERE oi.order_id IN ("#,
    );

    {
        let mut separated = query.separated(", ");
        for order_id in order_ids {
            separated.push_bind(order_id);
        }
    }

    query.push(
        r#")
               GROUP BY oi.product_id
           ) AS restored
           WHERE restored.product_id = p.id"#,
    );

    query.build().execute(&mut **tx).await?;
    Ok(())
}
