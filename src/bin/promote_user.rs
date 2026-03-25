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

    let email = "gecko.jeng@gmail.com";

    let result = sqlx::query("UPDATE users SET role = 'ADMIN' WHERE email = $1")
        .bind(email)
        .execute(&pool)
        .await
        .expect("Failed to update user role");

    println!(
        "✅ Updated {} rows. User {} is now ADMIN.",
        result.rows_affected(),
        email
    );
}
