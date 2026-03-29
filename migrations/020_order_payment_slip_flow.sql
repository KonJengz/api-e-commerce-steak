ALTER TABLE orders
    ADD COLUMN payment_slip_url TEXT,
    ADD COLUMN payment_slip_public_id TEXT,
    ADD COLUMN payment_submitted_at TIMESTAMPTZ;

CREATE INDEX idx_orders_payment_submitted_at
    ON orders(payment_submitted_at)
    WHERE payment_submitted_at IS NOT NULL;
