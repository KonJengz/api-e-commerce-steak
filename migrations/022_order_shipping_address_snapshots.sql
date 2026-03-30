ALTER TABLE orders
    ADD COLUMN shipping_recipient_name VARCHAR(255),
    ADD COLUMN shipping_phone VARCHAR(50),
    ADD COLUMN shipping_address_line TEXT,
    ADD COLUMN shipping_city VARCHAR(255),
    ADD COLUMN shipping_postal_code VARCHAR(50);

UPDATE orders AS o
SET shipping_recipient_name = a.recipient_name,
    shipping_phone = a.phone,
    shipping_address_line = a.address_line,
    shipping_city = a.city,
    shipping_postal_code = a.postal_code
FROM addresses AS a
WHERE o.shipping_address_id = a.id
  AND o.shipping_recipient_name IS NULL;
