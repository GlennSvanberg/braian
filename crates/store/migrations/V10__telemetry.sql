-- Task-level usage. Raw events keep an audit link (batch, seq, actor).
-- Aggregates in usage_tasks have no person columns. The usage API reads
-- only those aggregates.
--
-- These tables are not the change outbox. The relay does not publish them.

CREATE TABLE telemetry_settings (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    raw_retention_days integer NOT NULL DEFAULT 30 CHECK (raw_retention_days BETWEEN 1 AND 3650),
    inactivity_gap_seconds integer NOT NULL DEFAULT 300 CHECK (inactivity_gap_seconds BETWEEN 30 AND 86400)
);

INSERT INTO telemetry_settings (singleton) VALUES (true);

INSERT INTO view_types (key, title, builtin) VALUES ('usage', 'Usage', true)
ON CONFLICT (key) DO NOTHING;

CREATE TABLE telemetry_events (
    id uuid PRIMARY KEY,
    occurred_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    actor_type text NOT NULL CHECK (actor_type IN ('human', 'agent', 'integration')),
    task_id uuid NOT NULL,
    task_type text NOT NULL,
    session_id uuid NOT NULL,
    operation text NOT NULL,
    object_ids uuid[] NOT NULL DEFAULT '{}',
    object_count integer NOT NULL DEFAULT 0,
    result text NOT NULL,
    duration_ms integer,
    -- Join keys back to batches / changes. No foreign key: a telemetry insert
    -- must not lock or wait on the change log.
    audit_batch_id uuid,
    audit_seq bigint,
    actor_id uuid,
    on_behalf_of uuid,
    ambiguous boolean NOT NULL DEFAULT false,
    is_change boolean NOT NULL DEFAULT false
);

CREATE INDEX telemetry_events_occurred ON telemetry_events (occurred_at);
CREATE INDEX telemetry_events_task ON telemetry_events (task_id, occurred_at);

CREATE TABLE usage_tasks (
    id uuid PRIMARY KEY,
    task_type text NOT NULL,
    started_at timestamptz NOT NULL,
    ended_at timestamptz NOT NULL,
    human_time_ms bigint NOT NULL DEFAULT 0,
    changes_human integer NOT NULL DEFAULT 0,
    changes_agent integer NOT NULL DEFAULT 0,
    changes_integration integer NOT NULL DEFAULT 0,
    object_count integer NOT NULL DEFAULT 0,
    event_count integer NOT NULL DEFAULT 0,
    open boolean NOT NULL DEFAULT true,
    ambiguous boolean NOT NULL DEFAULT false,
    classifier text NOT NULL DEFAULT 'rules' CHECK (classifier IN ('rules', 'decisions'))
);

CREATE INDEX usage_tasks_started ON usage_tasks (started_at);
