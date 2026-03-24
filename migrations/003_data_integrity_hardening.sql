-- Tighten database-level invariants that were previously enforced only in application code.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM users
        GROUP BY lower(btrim(email))
        HAVING COUNT(*) > 1
    ) THEN
        RAISE EXCEPTION 'Cannot normalize users.email because duplicate emails exist when compared case-insensitively';
    END IF;
END $$;

UPDATE users
SET email = lower(btrim(email));

ALTER TABLE users
    ADD CONSTRAINT users_email_normalized CHECK (email = lower(btrim(email)));

WITH ranked_defaults AS (
    SELECT
        id,
        ROW_NUMBER() OVER (
            PARTITION BY user_id
            ORDER BY created_at DESC, id DESC
        ) AS row_num
    FROM addresses
    WHERE is_default
)
UPDATE addresses AS addresses_to_update
SET is_default = FALSE
FROM ranked_defaults
WHERE addresses_to_update.id = ranked_defaults.id
  AND ranked_defaults.row_num > 1;

CREATE UNIQUE INDEX idx_addresses_one_default_per_user
    ON addresses(user_id)
    WHERE is_default;

WITH ranked_verifications AS (
    SELECT
        id,
        user_id,
        ROW_NUMBER() OVER (
            PARTITION BY purpose, lower(btrim(email))
            ORDER BY created_at DESC, id DESC
        ) AS row_num_by_email,
        ROW_NUMBER() OVER (
            PARTITION BY purpose, user_id
            ORDER BY created_at DESC, id DESC
        ) AS row_num_by_user
    FROM email_verifications
)
DELETE FROM email_verifications
USING ranked_verifications
WHERE email_verifications.id = ranked_verifications.id
  AND (
      ranked_verifications.row_num_by_email > 1
      OR (
          ranked_verifications.user_id IS NOT NULL
          AND ranked_verifications.row_num_by_user > 1
      )
  );

UPDATE email_verifications
SET email = lower(btrim(email));

ALTER TABLE email_verifications
    ADD CONSTRAINT email_verifications_email_normalized CHECK (email = lower(btrim(email)));

CREATE UNIQUE INDEX idx_email_verifications_unique_purpose_email
    ON email_verifications(purpose, email);

CREATE UNIQUE INDEX idx_email_verifications_unique_purpose_user
    ON email_verifications(purpose, user_id)
    WHERE user_id IS NOT NULL;

ALTER TABLE orders
    DROP CONSTRAINT orders_shipping_address_id_fkey;

ALTER TABLE orders
    ADD CONSTRAINT orders_shipping_address_id_fkey
    FOREIGN KEY (shipping_address_id)
    REFERENCES addresses(id)
    ON DELETE SET NULL;
