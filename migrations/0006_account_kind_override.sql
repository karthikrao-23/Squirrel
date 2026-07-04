-- A user's manual override of an account's tax classification (taxable vs
-- retirement). NULL = derive automatically from Plaid's `subtype`
-- (AccountKind::from_subtype), which is the default. 'taxable'/'retirement' pin
-- the kind explicitly, for when Plaid's subtype is wrong or missing — e.g. a
-- 401(k) that arrives with a generic "brokerage" subtype and would otherwise be
-- treated as taxable (harvested, taxed if sold).
--
-- The account upsert (re-sync) lists its columns explicitly and never writes this
-- one, so a user's correction survives future Plaid syncs.
ALTER TABLE accounts
    ADD COLUMN kind_override TEXT
    CHECK (kind_override IN ('taxable', 'retirement'));
