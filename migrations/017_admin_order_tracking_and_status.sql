ALTER TABLE orders
    ADD COLUMN tracking_number VARCHAR(100),
    ADD COLUMN updated_at TIMESTAMPTZ;

UPDATE orders
SET updated_at = created_at
WHERE updated_at IS NULL;

ALTER TABLE orders
    ALTER COLUMN updated_at SET NOT NULL,
    ALTER COLUMN updated_at SET DEFAULT NOW();

CREATE INDEX idx_orders_created_at ON orders(created_at);
CREATE INDEX idx_orders_status_created_at ON orders(status, created_at DESC);
