-- Security hardening for auth/session/order flows.
-- Existing refresh tokens and verification codes are invalidated because they were stored in clear text.

DELETE FROM refresh_tokens;
DELETE FROM email_verifications;

ALTER TABLE products ADD CONSTRAINT products_stock_nonnegative CHECK (stock >= 0);
ALTER TABLE products ADD CONSTRAINT products_current_price_nonnegative CHECK (current_price >= 0);
ALTER TABLE order_items ADD CONSTRAINT order_items_quantity_positive CHECK (quantity > 0);
