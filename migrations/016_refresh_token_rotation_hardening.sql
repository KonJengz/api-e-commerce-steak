ALTER TABLE refresh_tokens
    ADD COLUMN family_id UUID,
    ADD COLUMN parent_token_id UUID REFERENCES refresh_tokens(id) ON DELETE SET NULL,
    ADD COLUMN replaced_by_token_id UUID REFERENCES refresh_tokens(id) ON DELETE SET NULL,
    ADD COLUMN consumed_at TIMESTAMPTZ,
    ADD COLUMN revoked_at TIMESTAMPTZ;

UPDATE refresh_tokens
SET family_id = id
WHERE family_id IS NULL;

ALTER TABLE refresh_tokens
    ALTER COLUMN family_id SET NOT NULL;

CREATE INDEX idx_refresh_tokens_family
    ON refresh_tokens(family_id);

CREATE INDEX idx_refresh_tokens_parent_token
    ON refresh_tokens(parent_token_id);

CREATE UNIQUE INDEX idx_refresh_tokens_replaced_by
    ON refresh_tokens(replaced_by_token_id)
    WHERE replaced_by_token_id IS NOT NULL;

CREATE UNIQUE INDEX idx_refresh_tokens_active_family
    ON refresh_tokens(family_id)
    WHERE consumed_at IS NULL AND revoked_at IS NULL;
