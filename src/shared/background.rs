use std::time::Duration;

use sqlx::PgPool;
use tokio::task::JoinHandle;

use crate::config::AppConfig;

use super::errors::AppError;
use super::maintenance::spawn_expired_data_cleanup;

pub struct BackgroundJobs {
    #[allow(dead_code)]
    cleanup: JoinHandle<()>,
}

pub fn start_background_jobs(pool: PgPool, config: AppConfig) -> BackgroundJobs {
    tracing::info!(
        cleanup_interval_minutes = config.cleanup_interval_minutes,
        product_image_upload_ttl_minutes = config.product_image_upload_ttl_minutes,
        "starting background jobs"
    );

    let cleanup = spawn_expired_data_cleanup(
        pool,
        config.clone(),
        Duration::from_secs(config.cleanup_interval_minutes * 60),
    );

    BackgroundJobs { cleanup }
}

pub fn spawn_app_task<F>(task_name: &'static str, future: F)
where
    F: std::future::Future<Output = Result<(), AppError>> + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(error) = future.await {
            tracing::error!(task = task_name, error = %error, "background task failed");
        }
    });
}
