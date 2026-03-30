ALTER TABLE products
    ADD COLUMN slug VARCHAR(255);

WITH normalized_products AS (
    SELECT
        p.id,
        COALESCE(
            NULLIF(
                LEFT(
                    BTRIM(
                        REGEXP_REPLACE(
                            LOWER(REGEXP_REPLACE(p.name, '[^[:alnum:]]+', '-', 'g')),
                            '-+',
                            '-',
                            'g'
                        ),
                        '-'
                    ),
                    120
                ),
                ''
            ),
            'product'
        ) AS base_slug,
        ROW_NUMBER() OVER (
            PARTITION BY COALESCE(
                NULLIF(
                    LEFT(
                        BTRIM(
                            REGEXP_REPLACE(
                                LOWER(REGEXP_REPLACE(p.name, '[^[:alnum:]]+', '-', 'g')),
                                '-+',
                                '-',
                                'g'
                            ),
                            '-'
                        ),
                        120
                    ),
                    ''
                ),
                'product'
            )
            ORDER BY p.created_at ASC, p.id ASC
        ) AS slug_rank
    FROM products p
),
assigned_product_slugs AS (
    SELECT
        id,
        CASE
            WHEN slug_rank = 1 THEN base_slug
            ELSE RTRIM(
                LEFT(base_slug, GREATEST(1, 120 - CHAR_LENGTH(slug_rank::text) - 1)),
                '-'
            ) || '-' || slug_rank::text
        END AS slug
    FROM normalized_products
)
UPDATE products p
SET slug = assigned.slug
FROM assigned_product_slugs assigned
WHERE assigned.id = p.id;

ALTER TABLE products
    ALTER COLUMN slug SET NOT NULL;

CREATE UNIQUE INDEX idx_products_slug_unique
    ON products(slug);

CREATE TABLE product_slug_history (
    slug VARCHAR(255) PRIMARY KEY,
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_product_slug_history_product_id
    ON product_slug_history(product_id);

ALTER TABLE categories
    ADD COLUMN slug VARCHAR(255);

WITH normalized_categories AS (
    SELECT
        c.id,
        COALESCE(
            NULLIF(
                LEFT(
                    BTRIM(
                        REGEXP_REPLACE(
                            LOWER(REGEXP_REPLACE(c.name, '[^[:alnum:]]+', '-', 'g')),
                            '-+',
                            '-',
                            'g'
                        ),
                        '-'
                    ),
                    120
                ),
                ''
            ),
            'category'
        ) AS base_slug,
        ROW_NUMBER() OVER (
            PARTITION BY COALESCE(
                NULLIF(
                    LEFT(
                        BTRIM(
                            REGEXP_REPLACE(
                                LOWER(REGEXP_REPLACE(c.name, '[^[:alnum:]]+', '-', 'g')),
                                '-+',
                                '-',
                                'g'
                            ),
                            '-'
                        ),
                        120
                    ),
                    ''
                ),
                'category'
            )
            ORDER BY c.created_at ASC, c.id ASC
        ) AS slug_rank
    FROM categories c
),
assigned_category_slugs AS (
    SELECT
        id,
        CASE
            WHEN slug_rank = 1 THEN base_slug
            ELSE RTRIM(
                LEFT(base_slug, GREATEST(1, 120 - CHAR_LENGTH(slug_rank::text) - 1)),
                '-'
            ) || '-' || slug_rank::text
        END AS slug
    FROM normalized_categories
)
UPDATE categories c
SET slug = assigned.slug
FROM assigned_category_slugs assigned
WHERE assigned.id = c.id;

ALTER TABLE categories
    ALTER COLUMN slug SET NOT NULL;

CREATE UNIQUE INDEX idx_categories_slug_unique
    ON categories(slug);

CREATE TABLE category_slug_history (
    slug VARCHAR(255) PRIMARY KEY,
    category_id UUID NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_category_slug_history_category_id
    ON category_slug_history(category_id);
