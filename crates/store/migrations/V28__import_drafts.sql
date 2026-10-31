-- An import is a draft: the proposal header lives in `proposals`, the rows stay
-- in `import_rows`. The session link and counters mark a proposal as an import.
-- The link is cleared if the session is purged; the janitor then discards the
-- proposal.

ALTER TABLE proposals
    ADD COLUMN session_id uuid REFERENCES import_sessions (id) ON DELETE SET NULL,
    ADD COLUMN counters jsonb;

CREATE INDEX proposals_session ON proposals (session_id) WHERE session_id IS NOT NULL;
CREATE INDEX proposals_open_created ON proposals (status, created_at DESC);

ALTER TABLE import_sessions ALTER COLUMN counters SET DEFAULT
    '{"new":0,"changed":0,"unchanged":0,"failed":0,"stale":0,"excluded":0,"processed":0,"total":0}';
UPDATE import_sessions SET counters = counters - 'delete';

-- Imports never delete. `excluded` rows were deselected at approval; `stale`
-- rows changed in the catalog after the preview and are not written.
DELETE FROM import_rows WHERE status = 'delete';
ALTER TABLE import_rows DROP CONSTRAINT import_rows_status_check;
ALTER TABLE import_rows ADD CONSTRAINT import_rows_status_check
    CHECK (status IN ('new', 'changed', 'failed', 'excluded', 'stale'));
