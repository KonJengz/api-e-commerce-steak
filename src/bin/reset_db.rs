use sqlx::postgres::PgPoolOptions;
use std::fs;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    println!("🗑️  Dropping all tables...");
    sqlx::query("DROP SCHEMA public CASCADE")
        .execute(&pool)
        .await
        .expect("Failed to drop schema");

    sqlx::query("CREATE SCHEMA public")
        .execute(&pool)
        .await
        .expect("Failed to create schema");

    println!("📦 Running migration...");
    let sql = fs::read_to_string("migrations/001_initial_schema.sql")
        .expect("Failed to read migration file");

    sqlx::raw_sql(&sql)
        .execute(&pool)
        .await
        .expect("Failed to run migration");

    println!("✅ Database reset complete!");
}
