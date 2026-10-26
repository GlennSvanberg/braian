-- Drop the old logical-replication slot before lowering wal_level.
-- Postgres refuses to start if a logical slot exists and wal_level is not logical.
-- Stop `braian` first so the slot is idle, then:
--   psql "$DATABASE_URL" -f deploy/drop-relay-slot.sql
-- After this, `docker compose up` with the current compose.yaml is safe.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_replication_slots
        WHERE slot_name = 'braian_relay' AND active
    ) THEN
        RAISE EXCEPTION 'slot braian_relay is active; stop braian, then run this again';
    END IF;
    IF EXISTS (
        SELECT 1 FROM pg_replication_slots WHERE slot_name = 'braian_relay'
    ) THEN
        PERFORM pg_drop_replication_slot('braian_relay');
    END IF;
END $$;

DROP PUBLICATION IF EXISTS braian_changes;
