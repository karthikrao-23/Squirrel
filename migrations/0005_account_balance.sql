-- Store Plaid's authoritative per-account dollar value (balances.current). For
-- accounts Plaid won't share holdings for (e.g. Fidelity BrokerageLink), this is
-- the only trustworthy value — we anchor the account's value to it instead of a
-- transaction-window estimate. Nullable: not every account reports a balance.
ALTER TABLE accounts ADD COLUMN current_balance NUMERIC;
