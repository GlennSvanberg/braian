-- Set by the relay when its replication slot was invalidated and the change
-- stream has a gap. The indexer rebuilds the search alias from Postgres.

ALTER TABLE indexer_progress
    ADD COLUMN reindex_requested boolean NOT NULL DEFAULT false;
