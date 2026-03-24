use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use super::errors::AppError;

#[derive(Clone, Copy)]
pub struct RateLimitRule {
    pub max_attempts: u32,
    pub window: Duration,
    pub scope: &'static str,
}

#[derive(Clone, Default)]
pub struct RateLimiter {
    entries: Arc<Mutex<HashMap<String, RateLimitEntry>>>,
}

#[derive(Clone, Copy)]
struct RateLimitEntry {
    count: u32,
    reset_at: Instant,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn check(&self, key: impl Into<String>, rule: RateLimitRule) -> Result<(), AppError> {
        let now = Instant::now();
        let key = key.into();
        let mut entries = self.entries.lock().await;

        if entries.len() > 4_096 {
            entries.retain(|_, entry| entry.reset_at > now);
        }

        let entry = entries.entry(key).or_insert(RateLimitEntry {
            count: 0,
            reset_at: now + rule.window,
        });

        if entry.reset_at <= now {
            *entry = RateLimitEntry {
                count: 0,
                reset_at: now + rule.window,
            };
        }

        if entry.count >= rule.max_attempts {
            return Err(AppError::TooManyRequests(format!(
                "Too many {} attempts. Please try again later.",
                rule.scope
            )));
        }

        entry.count += 1;
        Ok(())
    }
}
