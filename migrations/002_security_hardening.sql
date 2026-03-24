-- Security hardening for auth/session/order flows.
-- Existing refresh tokens and verification codes are invalidated because they were stored in clear text.

DELETE FROM refresh_tokens;
DELETE FROM email_verifications;

ALTER TABLE refresh_tokens DROP CONSTRAINT refresh_tokens_token_key;
ALTER TABLE refresh_tokens ADD COLUMN token_hash VARCHAR(64);
ALTER TABLE refresh_tokens ALTER COLUMN token_hash SET NOT NULL;
ALTER TABLE refresh_tokens DROP COLUMN token;

DROP INDEX IF EXISTS idx_refresh_tokens_token;
CREATE UNIQUE INDEX idx_refresh_tokens_token_hash ON refresh_tokens(token_hash);

ALTER TABLE email_verifications ADD COLUMN code_hash VARCHAR(64);
ALTER TABLE email_verifications ADD COLUMN purpose VARCHAR(32) NOT NULL DEFAULT 'REGISTER';
ALTER TABLE email_verifications ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE email_verifications ADD COLUMN attempt_count INT NOT NULL DEFAULT 0;
ALTER TABLE email_verifications ALTER COLUMN code_hash SET NOT NULL;
ALTER TABLE email_verifications DROP COLUMN code;
ALTER TABLE email_verifications
    ADD CONSTRAINT email_verifications_attempt_count_nonnegative CHECK (attempt_count >= 0);

CREATE INDEX idx_email_verifications_email_purpose ON email_verifications(email, purpose);
CREATE INDEX idx_email_verifications_user_id ON email_verifications(user_id);

ALTER TABLE products ADD CONSTRAINT products_stock_nonnegative CHECK (stock >= 0);
ALTER TABLE products ADD CONSTRAINT products_current_price_nonnegative CHECK (current_price >= 0);
ALTER TABLE order_items ADD CONSTRAINT order_items_quantity_positive CHECK (quantity > 0);
