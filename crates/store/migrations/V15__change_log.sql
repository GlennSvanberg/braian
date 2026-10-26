-- The change log is `changes` itself. Indexers claim seq ranges and advance
-- one cursor per lane. Rows in `changes` are never updated, so the audit
-- table does not churn and the claim table stays the size of in-flight work.

CREATE TABLE index_lane (
    lane text PRIMARY KEY CHECK (lane IN ('interactive', 'bulk')),
    committed_seq bigint NOT NULL
);

INSERT INTO index_lane (lane, committed_seq)
SELECT 'interactive', seq FROM indexer_progress WHERE id = 1
UNION ALL
SELECT 'bulk', seq FROM indexer_progress WHERE id = 1;

CREATE TABLE index_claim (
    lane text NOT NULL REFERENCES index_lane (lane),
    from_seq bigint NOT NULL,
    through_seq bigint NOT NULL,
    holder uuid NOT NULL,
    leased_until timestamptz NOT NULL,
    done boolean NOT NULL DEFAULT false,
    PRIMARY KEY (lane, from_seq),
    CHECK (through_seq > from_seq)
);

-- Lane reads are "kind = ? AND seq > cursor". The audit rows stay append-only.
CREATE INDEX changes_kind_seq ON changes (kind, seq);

-- One notify per transaction. A partitioned parent cannot carry a statement
-- trigger, and a statement trigger on a partition does not fire when the
-- insert is routed from the parent. The row trigger's WHEN clause skips every
-- row after the first in the transaction, so a bulk COPY still notifies once.
-- The payload is empty: listeners re-read by seq.
CREATE OR REPLACE FUNCTION braian_notify_changes() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM set_config('braian.changes_notified', '1', true);
    PERFORM pg_notify('braian_changes', '');
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION braian_attach_change_notify(part regclass) RETURNS void
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_trigger
        WHERE tgrelid = part
          AND tgname = 'braian_changes_notify'
          AND NOT tgisinternal
    ) THEN
        EXECUTE format(
            'CREATE TRIGGER braian_changes_notify AFTER INSERT ON %s FOR EACH ROW WHEN (current_setting(''braian.changes_notified'', true) IS DISTINCT FROM ''1'') EXECUTE FUNCTION braian_notify_changes()',
            part::text
        );
    END IF;
END;
$$;

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
        PERFORM braian_attach_change_notify(part_name::regclass);
    END LOOP;
END;
$$;

DO $$
DECLARE
    part regclass;
BEGIN
    FOR part IN
        SELECT inhrelid FROM pg_inherits WHERE inhparent = 'changes'::regclass
    LOOP
        PERFORM braian_attach_change_notify(part);
    END LOOP;
END $$;

-- V3 creates `braian_changes`. When wal_level is not logical that statement
-- warns and still creates the publication. Nothing reads it anymore.
DROP PUBLICATION IF EXISTS braian_changes;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_replication_slots WHERE slot_name = 'braian_relay') THEN
        PERFORM pg_drop_replication_slot('braian_relay');
    END IF;
EXCEPTION
    WHEN undefined_object OR object_in_use THEN
        RAISE NOTICE 'could not drop replication slot braian_relay: %', SQLERRM;
END $$;
