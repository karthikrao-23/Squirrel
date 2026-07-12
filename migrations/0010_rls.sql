-- Row-Level Security: database-enforced tenant isolation.
--
-- Until now, isolation was purely application-enforced — every query had to
-- remember `WHERE user_id = $1`. A single query that forgot the filter would
-- leak across tenants. RLS moves the guarantee into Postgres: each request runs
-- inside a transaction that sets `app.user_id` (see `db::begin_as_user`), and
-- these policies restrict every row a query can see or write to that user.
--
-- Fail-closed: a tenant query run without the setting matches no rows rather
-- than leaking another tenant's data. We read the GUC as
-- `NULLIF(current_setting('app.user_id', true), '')::uuid` — `missing_ok = true`
-- yields NULL when the parameter was never set, and NULLIF maps the EMPTY STRING
-- (what a custom GUC reverts to after a `SET LOCAL` on a pooled connection ends)
-- to NULL too. Without the NULLIF, that reverted `''` would raise a uuid cast
-- error instead of quietly matching nothing.
--
-- Bypass: genuinely cross-tenant system work (the Plaid per-app capacity count
-- that spans all users; the webhook path that arrives with no authenticated
-- user) runs via `db::begin_system`, which sets `app.rls_bypass = 'on'`. That
-- GUC is only ever set from server-controlled code, never from user input.
--
-- FORCE is included so the table *owner* is subject to the policies too — local
-- dev connects as the schema owner, and without FORCE owners bypass RLS. In
-- production the DML-only runtime role isn't the owner, so ENABLE alone already
-- binds it; FORCE is harmless there.
--
-- Scope: the seven user-owned tables. `users`/`sessions` are looked up before we
-- know the tenant (login by email, session by token hash), and `securities` is
-- shared public market data with no `user_id` — none get a policy.

DO $$
DECLARE
    t text;
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'plaid_items',
        'accounts',
        'holdings',
        'transactions',
        'tax_lots',
        'alerts',
        'portfolio_snapshots'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format(
            'CREATE POLICY tenant_isolation ON %I
               USING (
                   current_setting(''app.rls_bypass'', true) = ''on''
                   OR user_id = NULLIF(current_setting(''app.user_id'', true), '''')::uuid
               )
               WITH CHECK (
                   current_setting(''app.rls_bypass'', true) = ''on''
                   OR user_id = NULLIF(current_setting(''app.user_id'', true), '''')::uuid
               )',
            t
        );
    END LOOP;
END $$;
