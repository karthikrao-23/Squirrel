-- Authentication + tenant-isolation hardening.
--
-- This migration turns the single-user MVP into a real multi-tenant app:
--   1. Passwords (argon2id PHC strings) on `users`.
--   2. A DB-backed opaque-token session store (we store only the SHA-256 of the
--      token, never the token itself).
--   3. Re-keys the Plaid uniques so two users connecting the *same* institution
--      (identical Plaid ids — guaranteed in sandbox) cannot clobber each other's
--      rows. Previously these uniques were global, so user B's sync silently
--      overwrote user A's data.

-- --- Users: passwords + a real (NOT NULL) email -----------------------------

-- Nullable, no placeholder backfill. A `NOT NULL DEFAULT ''` would create
-- accounts whose hash is the empty string; the login path must treat NULL/empty
-- as "cannot authenticate", so we never want a magic empty value floating around.
ALTER TABLE users ADD COLUMN password_hash TEXT;

-- `ensure_default` (single-user MVP) could have produced one *or more* rows with
-- a NULL email across dev usage. We can't keep them once email is the login key,
-- and there's no password to migrate them to anyway. Deleting cascades to their
-- Plaid items/accounts/holdings/transactions/lots/alerts — acceptable for dev;
-- the production database is fresh.
DELETE FROM users WHERE email IS NULL;
ALTER TABLE users ALTER COLUMN email SET NOT NULL;

-- --- Sessions ----------------------------------------------------------------

-- Opaque random token → cookie; only its SHA-256 lives here. A DB leak therefore
-- never yields a usable session token. `expires_at` encodes both the sliding
-- window and the absolute cap (computed in app code); the engine reaps expired
-- rows on its periodic cycle.
CREATE TABLE sessions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash   BYTEA NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL
);
CREATE UNIQUE INDEX idx_sessions_token_hash ON sessions(token_hash);
CREATE INDEX idx_sessions_user ON sessions(user_id);

-- --- Tenant isolation: scope the Plaid uniques to the owning user ------------

-- These were globally UNIQUE on Plaid's id. Sandbox mints identical ids for
-- every user, so the global unique made user B's upsert collide with — and
-- overwrite — user A's row. Re-key each to (user_id, plaid_*_id) so the same
-- Plaid id can coexist across users while staying idempotent per user.
ALTER TABLE plaid_items DROP CONSTRAINT plaid_items_plaid_item_id_key;
ALTER TABLE plaid_items ADD CONSTRAINT plaid_items_user_item_key
    UNIQUE (user_id, plaid_item_id);

ALTER TABLE accounts DROP CONSTRAINT accounts_plaid_account_id_key;
ALTER TABLE accounts ADD CONSTRAINT accounts_user_account_key
    UNIQUE (user_id, plaid_account_id);

ALTER TABLE transactions DROP CONSTRAINT transactions_plaid_investment_transaction_id_key;
ALTER TABLE transactions ADD CONSTRAINT transactions_user_txn_key
    UNIQUE (user_id, plaid_investment_transaction_id);

-- `securities` stays globally keyed by plaid_security_id: it holds only public
-- market data (tickers, prices), no user-owned rows, so cross-user sharing is
-- correct and intentional.
