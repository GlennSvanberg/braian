-- Who last wrote the object. The same transaction that inserts the change
-- also sets this column. Existing rows take the latest actor from the log.

ALTER TABLE objects
    ADD COLUMN updated_by uuid REFERENCES principals (id);

UPDATE objects AS object
SET updated_by = latest.actor
FROM (
    SELECT DISTINCT ON (object_id) object_id, actor
    FROM changes
    WHERE object_id IS NOT NULL
    ORDER BY object_id, seq DESC
) AS latest
WHERE object.id = latest.object_id;
