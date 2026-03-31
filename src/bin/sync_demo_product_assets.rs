#[path = "support/demo_seed.rs"]
mod demo_seed;

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL_DIRECT")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("DATABASE_URL_DIRECT or DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    demo_seed::sync_demo_product_assets(&pool)
        .await
        .expect("Failed to sync demo product assets");
}
