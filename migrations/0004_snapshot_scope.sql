-- Segment value snapshots by scope so retirement accounts can be charted as a
-- group, separate from the whole portfolio. Existing rows become scope 'total'.
ALTER TABLE portfolio_snapshots ADD COLUMN scope TEXT NOT NULL DEFAULT 'total';
ALTER TABLE portfolio_snapshots DROP CONSTRAINT portfolio_snapshots_pkey;
ALTER TABLE portfolio_snapshots ADD PRIMARY KEY (user_id, as_of, scope);
