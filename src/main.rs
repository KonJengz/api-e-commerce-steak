mod address;
mod auth;
mod config;
mod order;
mod product;
mod shared;
mod user;

use axum::Router;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use config::AppConfig;

/// Shared application state accessible by all handlers
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: AppConfig,
}

#[tokio::main]
async fn main() {
    // Load .env file
    dotenvy::dotenv().ok();

    // Initialize tracing (logging)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend_rust_2=debug,tower_http=debug".into()),
        )
        .init();

    // Load config
    let config = AppConfig::from_env();
    let port = config.app_port;

    // Create database pool
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .expect("Failed to connect to database");

    tracing::info!("Connected to database");

    // CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // App state
    let state = AppState { pool, config };

    // Build router
    let app = Router::new()
        .nest("/api/auth", auth::handler::router())
        .nest("/api/users", user::handler::router())
        .nest("/api/addresses", address::handler::router())
        .nest("/api/products", product::handler::router())
        .nest("/api/orders", order::handler::router())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("Failed to bind to port");

    tracing::info!("🚀 Server running on http://localhost:{}", port);

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}
