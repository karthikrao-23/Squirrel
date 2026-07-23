-- When we last SUCCESSFULLY refreshed this account's data from the brokerage
-- (holdings + transactions pulled and tax lots rebuilt). Written only by the
-- sync path (see api::sync::sync_item), so unlike `updated_at` — which also
-- bumps on manual edits such as a tax-classification override — this is a
-- trustworthy "last refreshed from <institution>" timestamp for the UI.
--
-- Nullable: existing accounts (and a freshly linked one that hasn't completed a
-- sync yet) have no recorded sync time until the first successful refresh.
ALTER TABLE accounts ADD COLUMN last_synced_at TIMESTAMPTZ;
