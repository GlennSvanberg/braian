-- Latest-value reads used `DISTINCT ON (part) ... ORDER BY version DESC`.
-- That plan sorts every historical JSONB document to keep one row per part.
-- `is_latest` marks the current row so a GET is an index lookup of that row,
-- the same shape as the primary key before versions existed.

ALTER TABLE object_values ADD COLUMN is_latest boolean NOT NULL DEFAULT true;

UPDATE object_values AS older
SET is_latest = false
FROM (
    SELECT object_id, part, MAX(version) AS version
    FROM object_values
    GROUP BY object_id, part
) AS latest
WHERE older.object_id = latest.object_id
  AND older.part = latest.part
  AND older.version < latest.version;

CREATE UNIQUE INDEX object_values_current
    ON object_values (object_id, part)
    WHERE is_latest;

-- The descending index only existed to feed the DISTINCT ON sort.
DROP INDEX object_values_latest;
