-- Daily portfolio value snapshots: one row per (user, day), recorded by the
-- periodic cycle. Backs the dashboard's value-over-time chart.
CREATE TABLE portfolio_snapshots (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    as_of DATE NOT NULL,
    market_value NUMERIC NOT NULL,
    cost_basis NUMERIC NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, as_of)
);
