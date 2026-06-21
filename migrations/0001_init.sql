-- Initial schema for the investment tracker.
--
-- Single-user MVP, but every user-owned table carries `user_id` from day one so
-- enabling multi-tenant later is a constraint/auth change, not a rewrite.
-- All monetary values use NUMERIC (never floats). Postgres 13+ ships
-- gen_random_uuid() in core, so no extension is required.

CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email           TEXT UNIQUE,
    -- 'single' | 'married_filing_jointly' | 'married_filing_separately' | 'head_of_household'
    filing_status   TEXT NOT NULL DEFAULT 'single',
    taxable_income  NUMERIC(14, 2) NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE plaid_items (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plaid_item_id        TEXT NOT NULL UNIQUE,
    -- Encrypted at the application layer before being stored.
    access_token_encrypted BYTEA NOT NULL,
    institution_id       TEXT,
    institution_name     TEXT,
    transactions_cursor  TEXT,
    status               TEXT NOT NULL DEFAULT 'active',
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_plaid_items_user ON plaid_items(user_id);

CREATE TABLE accounts (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plaid_item_id    UUID NOT NULL REFERENCES plaid_items(id) ON DELETE CASCADE,
    plaid_account_id TEXT NOT NULL UNIQUE,
    name             TEXT NOT NULL,
    official_name    TEXT,
    type             TEXT,
    subtype          TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_accounts_user ON accounts(user_id);

-- Security data is shared across users (not user-owned), keyed by Plaid's id.
CREATE TABLE securities (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plaid_security_id  TEXT NOT NULL UNIQUE,
    ticker             TEXT,
    name               TEXT,
    cusip              TEXT,
    type               TEXT,
    close_price        NUMERIC(20, 6),
    close_price_as_of  DATE,
    currency           TEXT,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE holdings (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    account_id               UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    security_id              UUID NOT NULL REFERENCES securities(id) ON DELETE CASCADE,
    quantity                 NUMERIC(20, 8) NOT NULL,
    institution_price        NUMERIC(20, 6),
    institution_price_as_of  DATE,
    institution_value        NUMERIC(20, 6),
    cost_basis               NUMERIC(20, 6),
    currency                 TEXT,
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, security_id)
);
CREATE INDEX idx_holdings_user ON holdings(user_id);

CREATE TABLE transactions (
    id                              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    account_id                      UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    security_id                     UUID REFERENCES securities(id) ON DELETE SET NULL,
    plaid_investment_transaction_id TEXT NOT NULL UNIQUE,
    type                            TEXT,
    subtype                         TEXT,
    quantity                        NUMERIC(20, 8),
    price                           NUMERIC(20, 6),
    amount                          NUMERIC(20, 6),
    fees                            NUMERIC(20, 6),
    date                            DATE NOT NULL,
    name                            TEXT,
    currency                        TEXT,
    created_at                      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_transactions_user ON transactions(user_id);
CREATE INDEX idx_transactions_acct_sec_date ON transactions(account_id, security_id, date);

-- Derived per-purchase tax lots, reconstructed from the transactions feed.
CREATE TABLE tax_lots (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    account_id           UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    security_id          UUID NOT NULL REFERENCES securities(id) ON DELETE CASCADE,
    open_date            DATE NOT NULL,
    quantity             NUMERIC(20, 8) NOT NULL,
    remaining_quantity   NUMERIC(20, 8) NOT NULL,
    cost_basis_per_share NUMERIC(20, 6) NOT NULL,
    status               TEXT NOT NULL DEFAULT 'open',
    source_transaction_id UUID REFERENCES transactions(id) ON DELETE SET NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_tax_lots_user ON tax_lots(user_id);
CREATE INDEX idx_tax_lots_acct_sec ON tax_lots(account_id, security_id, status);

CREATE TABLE alerts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    type        TEXT NOT NULL,
    security_id UUID REFERENCES securities(id) ON DELETE SET NULL,
    title       TEXT NOT NULL,
    message     TEXT NOT NULL,
    payload     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    read_at     TIMESTAMPTZ,
    emailed_at  TIMESTAMPTZ
);
CREATE INDEX idx_alerts_user ON alerts(user_id, created_at DESC);
