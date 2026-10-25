-- List pages filter active objects by type and key, then order by id so a
-- cursor can resume without skipping or repeating a row. The partial indexes
-- match that plan: type equality plus id order, id order alone, and a trigram
-- index for case-insensitive key contains (`q`).

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX objects_active_id
    ON objects (id)
    INCLUDE (key, type_id, version)
    WHERE deleted_at IS NULL;

CREATE INDEX objects_active_type_id
    ON objects (type_id, id)
    INCLUDE (key, version)
    WHERE deleted_at IS NULL;

CREATE INDEX objects_active_key_trgm
    ON objects USING gin (key gin_trgm_ops)
    WHERE deleted_at IS NULL;
