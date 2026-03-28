CREATE INDEX idx_users_name_lower_pattern
    ON users ((LOWER(name)) text_pattern_ops);

CREATE INDEX idx_users_email_lower_pattern
    ON users ((LOWER(email)) text_pattern_ops);

CREATE INDEX idx_orders_tracking_number_lower_pattern
    ON orders ((LOWER(tracking_number)) text_pattern_ops)
    WHERE tracking_number IS NOT NULL;
