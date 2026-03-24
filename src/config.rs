use std::env;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_access_expiry_minutes: i64,
    pub jwt_refresh_expiry_days: i64,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub smtp_from: String,
    pub app_url: String,
    pub app_port: u16,
    pub google_client_id: String,
    pub github_client_id: String,
    pub github_client_secret: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
            jwt_secret: env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
            jwt_access_expiry_minutes: env::var("JWT_ACCESS_EXPIRY_MINUTES")
                .unwrap_or_else(|_| "15".to_string())
                .parse()
                .expect("JWT_ACCESS_EXPIRY_MINUTES must be a number"),
            jwt_refresh_expiry_days: env::var("JWT_REFRESH_EXPIRY_DAYS")
                .unwrap_or_else(|_| "7".to_string())
                .parse()
                .expect("JWT_REFRESH_EXPIRY_DAYS must be a number"),
            smtp_host: env::var("SMTP_HOST").expect("SMTP_HOST must be set"),
            smtp_port: env::var("SMTP_PORT")
                .unwrap_or_else(|_| "587".to_string())
                .parse()
                .expect("SMTP_PORT must be a number"),
            smtp_username: env::var("SMTP_USERNAME").expect("SMTP_USERNAME must be set"),
            smtp_password: env::var("SMTP_PASSWORD").expect("SMTP_PASSWORD must be set"),
            smtp_from: env::var("SMTP_FROM").expect("SMTP_FROM must be set"),
            app_url: env::var("APP_URL").unwrap_or_else(|_| "http://localhost:3000".to_string()),
            app_port: env::var("APP_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("APP_PORT must be a number"),
            google_client_id: env::var("GOOGLE_CLIENT_ID")
                .unwrap_or_else(|_| "your-google-client-id".to_string()),
            github_client_id: env::var("GITHUB_CLIENT_ID")
                .unwrap_or_else(|_| "your-github-client-id".to_string()),
            github_client_secret: env::var("GITHUB_CLIENT_SECRET")
                .unwrap_or_else(|_| "your-github-client-secret".to_string()),
        }
    }
}
