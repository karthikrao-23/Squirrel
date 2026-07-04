-- Plaid does NOT free an app's connection slot when an Item is removed — a
-- disconnected connection keeps counting toward the app's item cap. We hard-
-- delete the plaid_items row (and its data) on disconnect, so without a record
-- the removed connection would silently free capacity we don't actually have.
--
-- This tombstone table preserves that: an app's *used* connections =
-- active plaid_items + removed tombstones, per client_id. It stores no financial
-- data — just enough to count consumed slots and keep an audit trail.
CREATE TABLE plaid_removed_items (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Kept for audit; the connection stays consumed on Plaid even if the user is
    -- later deleted, so don't cascade the tombstone away.
    user_id          UUID REFERENCES users(id) ON DELETE SET NULL,
    plaid_client_id  TEXT NOT NULL,
    plaid_item_id    TEXT,
    institution_name TEXT,
    removed_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_plaid_removed_client ON plaid_removed_items(plaid_client_id);
