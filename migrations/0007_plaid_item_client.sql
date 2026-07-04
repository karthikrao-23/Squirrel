-- Which Plaid app (client_id) created this item. Plaid caps live items per app
-- (the "10 connections" limit on a Plaid account), so the app shards new
-- connections across several client_id/secret pairs. Syncing, removing, and
-- verifying webhooks for an item all require the SAME credentials that minted its
-- access token, so we record the client_id here.
--
-- Nullable: existing rows predate multi-app support and were all created by the
-- original (primary) client_id, so NULL resolves to the primary app at runtime.
ALTER TABLE plaid_items ADD COLUMN plaid_client_id TEXT;
