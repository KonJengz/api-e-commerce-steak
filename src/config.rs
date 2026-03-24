use std::env;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Production,
}

impl AppEnv {
    fn from_env(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "development" | "dev" => Self::Development,
            "production" | "prod" => Self::Production,
            other => panic!(
                "APP_ENV must be one of development|production, got {}",
                other
            ),
        }
    }

    pub fn is_production(&self) -> bool {
        matches!(self, Self::Production)
    }
}

impl fmt::Display for AppEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Development => write!(f, "development"),
            Self::Production => write!(f, "production"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub app_env: AppEnv,
    pub database_url: String,
    pub database_max_connections: u32,
    pub database_acquire_timeout_seconds: u64,
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
    pub cookie_secure: bool,
    pub trust_proxy_headers: bool,
    pub cleanup_interval_minutes: u64,
    pub product_image_upload_ttl_minutes: i64,
    pub log_json: bool,
    pub google_client_id: String,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub cloudinary_cloud_name: String,
    pub cloudinary_api_key: String,
    pub cloudinary_api_secret: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let app_env =
            AppEnv::from_env(&env::var("APP_ENV").unwrap_or_else(|_| "development".to_string()));
        let cookie_secure_default = app_env.is_production();
        let log_json_default = app_env.is_production();

        let config = Self {
            app_env,
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
            database_max_connections: parse_u32_env("DATABASE_MAX_CONNECTIONS", 10),
            database_acquire_timeout_seconds: parse_u64_env("DATABASE_ACQUIRE_TIMEOUT_SECONDS", 30),
            jwt_secret: env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
            jwt_access_expiry_minutes: parse_i64_env("JWT_ACCESS_EXPIRY_MINUTES", 15),
            jwt_refresh_expiry_days: parse_i64_env("JWT_REFRESH_EXPIRY_DAYS", 7),
            smtp_host: env::var("SMTP_HOST").expect("SMTP_HOST must be set"),
            smtp_port: parse_u16_env("SMTP_PORT", 587),
            smtp_username: env::var("SMTP_USERNAME").expect("SMTP_USERNAME must be set"),
            smtp_password: env::var("SMTP_PASSWORD").expect("SMTP_PASSWORD must be set"),
            smtp_from: env::var("SMTP_FROM").expect("SMTP_FROM must be set"),
            app_url: env::var("APP_URL").unwrap_or_else(|_| "http://localhost:3000".to_string()),
            app_port: parse_u16_env("APP_PORT", 3000),
            cookie_secure: parse_bool_env("COOKIE_SECURE", cookie_secure_default),
            trust_proxy_headers: parse_bool_env("TRUST_PROXY_HEADERS", false),
            cleanup_interval_minutes: parse_u64_env("CLEANUP_INTERVAL_MINUTES", 10),
            product_image_upload_ttl_minutes: parse_i64_env("PRODUCT_IMAGE_UPLOAD_TTL_MINUTES", 60),
            log_json: parse_bool_env("LOG_JSON", log_json_default),
            google_client_id: env::var("GOOGLE_CLIENT_ID")
                .unwrap_or_else(|_| "your-google-client-id".to_string()),
            github_client_id: env::var("GITHUB_CLIENT_ID")
                .unwrap_or_else(|_| "your-github-client-id".to_string()),
            github_client_secret: env::var("GITHUB_CLIENT_SECRET")
                .unwrap_or_else(|_| "your-github-client-secret".to_string()),
            cloudinary_cloud_name: env::var("CLOUDINARY_CLOUD_NAME")
                .unwrap_or_else(|_| "your-cloud-name".to_string()),
            cloudinary_api_key: env::var("CLOUDINARY_API_KEY")
                .unwrap_or_else(|_| "your-api-key".to_string()),
            cloudinary_api_secret: env::var("CLOUDINARY_API_SECRET")
                .unwrap_or_else(|_| "your-api-secret".to_string()),
        };

        config.validate();
        config
    }

    fn validate(&self) {
        assert!(
            self.jwt_secret.trim().len() >= 32,
            "JWT_SECRET must be at least 32 characters long"
        );
        assert!(
            self.jwt_access_expiry_minutes > 0,
            "JWT_ACCESS_EXPIRY_MINUTES must be greater than 0"
        );
        assert!(
            self.jwt_refresh_expiry_days > 0,
            "JWT_REFRESH_EXPIRY_DAYS must be greater than 0"
        );
        assert!(
            self.database_max_connections > 0,
            "DATABASE_MAX_CONNECTIONS must be greater than 0"
        );
        assert!(
            self.database_acquire_timeout_seconds > 0,
            "DATABASE_ACQUIRE_TIMEOUT_SECONDS must be greater than 0"
        );
        assert!(
            self.cleanup_interval_minutes > 0,
            "CLEANUP_INTERVAL_MINUTES must be greater than 0"
        );
        assert!(
            self.product_image_upload_ttl_minutes > 0,
            "PRODUCT_IMAGE_UPLOAD_TTL_MINUTES must be greater than 0"
        );

        if self.app_env.is_production() {
            assert!(
                self.app_url.starts_with("https://"),
                "APP_URL must use https:// in production"
            );
            assert!(
                self.cookie_secure,
                "COOKIE_SECURE must be true in production"
            );
            assert!(
                !self.jwt_secret.contains("change-this")
                    && !self.jwt_secret.contains("your-super-secret"),
                "JWT_SECRET must not use a placeholder value in production"
            );
            assert!(
                !self.cloudinary_cloud_name.contains("your-cloud-name")
                    && !self.cloudinary_api_key.contains("your-api-key")
                    && !self.cloudinary_api_secret.contains("your-api-secret"),
                "Cloudinary credentials must not use placeholder values in production"
            );
        }
    }
}

fn parse_bool_env(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "y" | "on" => true,
            "0" | "false" | "no" | "n" | "off" => false,
            _ => panic!("{} must be a boolean", name),
        },
        Err(_) => default,
    }
}

fn parse_i64_env(name: &str, default: i64) -> i64 {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .unwrap_or_else(|_| panic!("{} must be a number", name))
}

fn parse_u64_env(name: &str, default: u64) -> u64 {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .unwrap_or_else(|_| panic!("{} must be a number", name))
}

fn parse_u32_env(name: &str, default: u32) -> u32 {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .unwrap_or_else(|_| panic!("{} must be a number", name))
}

fn parse_u16_env(name: &str, default: u16) -> u16 {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .unwrap_or_else(|_| panic!("{} must be a number", name))
}
