-- Alerts now refresh in place: while a condition still holds, the periodic cycle
-- updates the existing unread alert's message/payload rather than leaving a
-- stale copy from first detection. Track when that last happened so the UI can
-- show a current "updated N minutes ago" instead of the original creation time.
--
-- Backfill existing rows to their created_at so ordering/relative-time stay sane.
ALTER TABLE alerts ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
UPDATE alerts SET updated_at = created_at;
