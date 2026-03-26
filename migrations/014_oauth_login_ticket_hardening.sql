DELETE FROM oauth_login_tickets;

ALTER TABLE oauth_login_tickets
    ADD COLUMN ticket_hash VARCHAR(255);

ALTER TABLE oauth_login_tickets
    ALTER COLUMN ticket_hash SET NOT NULL,
    DROP COLUMN access_token,
    DROP COLUMN refresh_token;

CREATE UNIQUE INDEX idx_oauth_login_tickets_ticket_hash
    ON oauth_login_tickets(ticket_hash);
