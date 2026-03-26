use std::time::Duration;

use crate::config::AppConfig;
use crate::shared::cloudinary;
use sqlx::PgPool;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

const UNVERIFIED_USER_RETENTION_DAYS: i64 = 7;

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
                        || stats.deleted_pending_product_images > 0
                        || stats.deleted_pending_product_image_deletions > 0
                    {
                        tracing::info!(
                            deleted_refresh_tokens = stats.deleted_refresh_tokens,
                            deleted_email_verifications = stats.deleted_email_verifications,
                            deleted_oauth_login_tickets = stats.deleted_oauth_login_tickets,
                            deleted_unverified_users = stats.deleted_unverified_users,
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
        deleted_pending_product_images,
        deleted_pending_product_image_deletions,
    })
}
