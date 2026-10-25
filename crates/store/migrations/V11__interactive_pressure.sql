-- In-flight interactive requests, one row per process.
-- Bulk writers in other processes read the fresh rows and yield.
-- A row that stops being updated is ignored, so a crashed process
-- does not leave imports paused.

CREATE TABLE interactive_pressure (
    instance_id uuid PRIMARY KEY,
    inflight integer NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
