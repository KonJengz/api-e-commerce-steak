mod address;
mod auth;
mod config;
mod order;
mod product;
mod shared;
mod user;

use axum::http::{header, HeaderValue, Method};
use axum::Router;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use config::AppConfig;
use shared::rate_limit::RateLimiter;

/// Shared application state accessible by all handlers
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: AppConfig,
    pub auth_rate_limiter: RateLimiter,
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
    let allowed_origin = config
        .app_url
        .parse::<HeaderValue>()
        .expect("APP_URL must be a valid origin");

    // Create database pool
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .expect("Failed to connect to database");

    tracing::info!("Connected to database");

    // CORS — credentials require explicit headers/methods (wildcard not allowed)
    let cors = CorsLayer::new()
        .allow_origin(allowed_origin)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::ACCEPT,
        ])
        .allow_credentials(true);

    // App state
    let state = AppState {
        pool,
        config,
        auth_rate_limiter: RateLimiter::new(),
    };

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

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("Server failed");
}
