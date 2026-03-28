UPDATE categories
SET name = BTRIM(name)
WHERE name <> BTRIM(name);

ALTER TABLE categories
    ADD CONSTRAINT categories_name_not_blank CHECK (CHAR_LENGTH(BTRIM(name)) > 0);

CREATE UNIQUE INDEX idx_categories_name_lower_unique
    ON categories (LOWER(name));
