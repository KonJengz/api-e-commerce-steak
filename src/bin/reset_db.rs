use sqlx::postgres::PgPoolOptions;
use std::fs;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    println!("Dropping and recreating public schema...");
    sqlx::query("DROP SCHEMA public CASCADE")
        .execute(&pool)
        .await
        .expect("Failed to drop schema");

    sqlx::query("CREATE SCHEMA public")
        .execute(&pool)
        .await
        .expect("Failed to create schema");

    println!("Running all migrations...");
    let mut migrations = fs::read_dir("migrations")
        .expect("Failed to read migrations directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("sql"))
        .collect::<Vec<PathBuf>>();

    migrations.sort();

    for migration in migrations {
        let path = migration.display().to_string();
        println!("Applying {}", path);
        let sql = fs::read_to_string(&migration).expect("Failed to read migration file");

        sqlx::raw_sql(&sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("Failed to run migration {}: {}", path, error));
    }

    println!("Database reset complete!");
}
