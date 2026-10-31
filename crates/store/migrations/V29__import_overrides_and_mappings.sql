-- Cell edits made in the Check step: { "<row_no>": { "<col>": "<text>" } }.
-- They survive a new diff of the same session.
ALTER TABLE import_sessions
    ADD COLUMN overrides jsonb NOT NULL DEFAULT '{}'::jsonb;

-- Saved column mappings, shared by everyone who can import.
CREATE TABLE import_mappings (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    type_key text NOT NULL,
    -- Normalised, sorted header names joined by a newline. Exact match reuses the mapping.
    header_signature text NOT NULL,
    headers jsonb NOT NULL DEFAULT '[]'::jsonb,
    mapping jsonb NOT NULL,
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX import_mappings_signature ON import_mappings (header_signature);

INSERT INTO view_types (key, title, builtin) VALUES
  ('import_mappings', 'Import mappings', true)
ON CONFLICT (key) DO NOTHING;
