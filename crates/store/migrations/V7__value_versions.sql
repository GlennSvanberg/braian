-- Version 1 of an object's parts is the create snapshot. Later edits insert a
-- new version instead of rewriting those rows, so history can rebuild the
-- create diff without storing a second copy of the values in `changes`.

ALTER TABLE object_values ADD COLUMN version bigint NOT NULL DEFAULT 1;

ALTER TABLE object_values DROP CONSTRAINT object_values_pkey;

ALTER TABLE object_values ADD PRIMARY KEY (object_id, part, version);

-- Latest-value reads are `DISTINCT ON (object_id, part) ... ORDER BY version DESC`.
-- The primary key is ascending, so that order sorts. This index matches it.
CREATE INDEX object_values_latest ON object_values (object_id, part, version DESC);
