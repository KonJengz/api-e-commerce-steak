CREATE TABLE pending_product_images (
    public_id VARCHAR(255) PRIMARY KEY,
    secure_url TEXT NOT NULL,
    uploaded_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pending_product_images_uploaded_by
ON pending_product_images(uploaded_by);

CREATE INDEX idx_pending_product_images_expires_at
ON pending_product_images(expires_at);
