-- Unchanged rows are kept in the draft so they can be listed.
ALTER TABLE import_rows DROP CONSTRAINT import_rows_status_check;
ALTER TABLE import_rows ADD CONSTRAINT import_rows_status_check
    CHECK (status IN ('new', 'changed', 'failed', 'excluded', 'stale', 'unchanged'));
