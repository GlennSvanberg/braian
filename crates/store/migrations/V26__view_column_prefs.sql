-- Per-user column preferences for one view instance (field picker, step 3).
-- Idempotent: safe to apply on a database that already has data; only adds.

CREATE TABLE IF NOT EXISTS view_column_prefs (
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    environment text NOT NULL DEFAULT 'default',
    view_id uuid NOT NULL REFERENCES views (id) ON DELETE CASCADE,
    document jsonb NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, environment, view_id)
);

CREATE INDEX IF NOT EXISTS view_column_prefs_view ON view_column_prefs (view_id);
