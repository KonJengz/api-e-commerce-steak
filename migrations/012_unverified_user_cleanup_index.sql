CREATE INDEX idx_users_unverified_updated_at
    ON users(updated_at)
    WHERE is_verified = FALSE;
