CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE product_images (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    image_url TEXT NOT NULL,
    image_public_id VARCHAR(255) NOT NULL UNIQUE,
    sort_order INT NOT NULL DEFAULT 0,
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_product_images_product
ON product_images(product_id);

CREATE INDEX idx_product_images_product_order
ON product_images(product_id, is_primary DESC, sort_order ASC, created_at ASC);

CREATE UNIQUE INDEX idx_product_images_one_primary
ON product_images(product_id)
WHERE is_primary;

INSERT INTO product_images (product_id, image_url, image_public_id, sort_order, is_primary)
SELECT id, image_url, image_public_id, 0, TRUE
FROM products
WHERE image_url IS NOT NULL
  AND image_public_id IS NOT NULL;
