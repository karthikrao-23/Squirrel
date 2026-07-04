-- Allow 'debt' as a third manual account classification. A debt/liability
-- account (loan, margin, credit line, …) is excluded from portfolio value and
-- the investment views — its balance shouldn't inflate holdings. Like the other
-- kinds it's a user override; there's no auto-detection from subtype.
ALTER TABLE accounts DROP CONSTRAINT accounts_kind_override_check;
ALTER TABLE accounts
    ADD CONSTRAINT accounts_kind_override_check
    CHECK (kind_override IN ('taxable', 'retirement', 'debt'));
