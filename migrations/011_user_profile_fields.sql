ALTER TABLE users
    ADD COLUMN name VARCHAR(100),
    ADD COLUMN image TEXT;

UPDATE users
SET name = LEFT(COALESCE(NULLIF(BTRIM(SPLIT_PART(email, '@', 1)), ''), 'User'), 100)
WHERE name IS NULL;

ALTER TABLE users
    ALTER COLUMN name SET NOT NULL,
    ADD CONSTRAINT users_name_not_blank CHECK (CHAR_LENGTH(BTRIM(name)) > 0);

ALTER TABLE email_verifications
    ADD COLUMN name VARCHAR(100),
    ADD COLUMN image TEXT;

UPDATE email_verifications
SET name = LEFT(COALESCE(NULLIF(BTRIM(SPLIT_PART(email, '@', 1)), ''), 'User'), 100)
WHERE purpose = 'REGISTER' AND name IS NULL;
