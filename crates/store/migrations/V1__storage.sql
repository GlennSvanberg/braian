-- Phase 2 storage. Object values live in object_values, one JSONB document per
-- part. The part key is what spike 1 chooses: one document, one part per
-- language, or one part per language and attribute group. The tables do not
-- change between those forms.

CREATE TABLE principals (
    id uuid PRIMARY KEY,
    kind text NOT NULL CHECK (kind IN ('human', 'agent', 'integration')),
    name text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE batches (
    id uuid PRIMARY KEY,
    actor uuid NOT NULL REFERENCES principals (id),
    on_behalf_of uuid REFERENCES principals (id),
    kind text NOT NULL CHECK (kind IN ('interactive', 'bulk')),
    source text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE definition_revisions (
    model_revision bigint PRIMARY KEY,
    definitions jsonb NOT NULL,
    batch_id uuid NOT NULL REFERENCES batches (id),
    created_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE definition_revisions ALTER COLUMN definitions SET COMPRESSION lz4;

CREATE TABLE objects (
    id uuid PRIMARY KEY,
    type_id uuid NOT NULL,
    key text NOT NULL,
    version bigint NOT NULL,
    model_revision bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz
);

-- A deleted key can be used again. The database, not the application, enforces uniqueness.
CREATE UNIQUE INDEX objects_type_id_key_active ON objects (type_id, key) WHERE deleted_at IS NULL;

CREATE TABLE object_values (
    object_id uuid NOT NULL REFERENCES objects (id),
    part text NOT NULL,
    "values" jsonb NOT NULL,
    PRIMARY KEY (object_id, part)
);

ALTER TABLE object_values ALTER COLUMN "values" SET COMPRESSION lz4;

CREATE TABLE object_structures (
    object_id uuid NOT NULL REFERENCES objects (id),
    structure_id uuid NOT NULL,
    group_id uuid NOT NULL,
    PRIMARY KEY (object_id, structure_id)
);

CREATE INDEX object_structures_group ON object_structures (structure_id, group_id);

-- Audit trail and outbox are the same rows. `at` is the partition key, so it
-- has to be part of the primary key. seq stays the order the relay will follow.
CREATE TABLE changes (
    seq bigint GENERATED ALWAYS AS IDENTITY,
    at timestamptz NOT NULL DEFAULT now(),
    batch_id uuid NOT NULL REFERENCES batches (id),
    object_id uuid,
    version bigint,
    op text NOT NULL CHECK (op IN ('create', 'update', 'delete', 'definitions')),
    actor uuid NOT NULL REFERENCES principals (id),
    on_behalf_of uuid REFERENCES principals (id),
    diff jsonb,
    PRIMARY KEY (seq, at)
) PARTITION BY RANGE (at);

ALTER TABLE changes ALTER COLUMN diff SET COMPRESSION lz4;

CREATE INDEX changes_object_seq ON changes (object_id, seq);

CREATE TABLE changes_default PARTITION OF changes DEFAULT;

-- Monthly partitions are created ahead of the writes. The default partition
-- catches a row that arrives before this function has been run for its month.
CREATE OR REPLACE FUNCTION ensure_change_partitions(months_ahead integer)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    month_start date;
    month_end date;
    part_name text;
    i integer;
BEGIN
    IF months_ahead < 0 THEN
        RAISE EXCEPTION 'months_ahead must be >= 0';
    END IF;
    FOR i IN 0..months_ahead LOOP
        month_start := (date_trunc('month', now()) + make_interval(months => i))::date;
        month_end := (month_start + interval '1 month')::date;
        part_name := 'changes_' || to_char(month_start, 'YYYY_MM');
        BEGIN
            EXECUTE format(
                'CREATE TABLE %I PARTITION OF changes FOR VALUES FROM (%L) TO (%L)',
                part_name,
                month_start,
                month_end
            );
        EXCEPTION
            WHEN duplicate_table THEN
                NULL;
        END;
    END LOOP;
END;
$$;

SELECT ensure_change_partitions(3);
