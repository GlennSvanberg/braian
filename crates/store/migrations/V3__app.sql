-- Product tables on top of the phase 2 log. `kind` is the lane (interactive or
-- bulk). A publication is created only when this server is already on logical
-- replication, so a fresh database at the default wal_level still migrates.
-- V15 drops that publication: the indexer reads `changes` by seq.

ALTER TABLE changes ADD COLUMN kind text NOT NULL DEFAULT 'interactive';

ALTER TABLE changes
    ADD CONSTRAINT changes_kind_check CHECK (kind IN ('interactive', 'bulk'));

DO $$
BEGIN
    IF current_setting('wal_level') = 'logical'
       AND NOT EXISTS (SELECT 1 FROM pg_publication WHERE pubname = 'braian_changes') THEN
        CREATE PUBLICATION braian_changes FOR TABLE changes
            WITH (publish = 'insert', publish_via_partition_root = true);
    END IF;
END $$;

CREATE TABLE users (
    id uuid PRIMARY KEY,
    principal_id uuid NOT NULL UNIQUE REFERENCES principals (id),
    email text NOT NULL UNIQUE,
    password_hash text NOT NULL,
    role text NOT NULL CHECK (role IN ('admin', 'editor', 'viewer')),
    display_name text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    token_hash bytea PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE api_tokens (
    token_hash bytea PRIMARY KEY,
    principal_id uuid NOT NULL REFERENCES principals (id),
    owner_user_id uuid NOT NULL REFERENCES users (id),
    name text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('agent', 'api')),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workspaces (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users (id),
    name text NOT NULL,
    locked boolean NOT NULL DEFAULT false,
    share_token text UNIQUE,
    layout jsonb NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workspace_members (
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (workspace_id, user_id)
);

CREATE TABLE view_types (
    key text PRIMARY KEY,
    title text NOT NULL,
    builtin boolean NOT NULL DEFAULT true,
    embed_url text
);

INSERT INTO view_types (key, title, builtin) VALUES
    ('structure_tree', 'Struktur', true),
    ('object_table', 'Objekt', true),
    ('object_detail', 'Detalj', true),
    ('graph', 'Graf', true),
    ('data_model', 'Datamodell', true),
    ('history', 'Historik', true),
    ('batch_log', 'Batchlogg', true),
    ('job_history', 'Jobb', true),
    ('dashboard', 'Översikt', true),
    ('proposal', 'Förslag', true),
    ('embed', 'Inbäddad vy', true);

CREATE TABLE views (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    type_key text NOT NULL REFERENCES view_types (key),
    title text NOT NULL,
    pane text NOT NULL DEFAULT 'main',
    position integer NOT NULL DEFAULT 0,
    state jsonb NOT NULL DEFAULT '{}'
);

CREATE TABLE assortments (
    id uuid PRIMARY KEY,
    owner_id uuid REFERENCES users (id),
    name text NOT NULL,
    query jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE job_types (
    key text PRIMARY KEY,
    title text NOT NULL,
    reads_fields text[] NOT NULL DEFAULT '{}',
    assortment_id uuid REFERENCES assortments (id)
);

CREATE TABLE job_runs (
    id uuid PRIMARY KEY,
    type_key text NOT NULL REFERENCES job_types (key),
    status text NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
    assortment_id uuid,
    actor uuid NOT NULL REFERENCES principals (id),
    on_behalf_of uuid REFERENCES principals (id),
    progress_done integer NOT NULL DEFAULT 0,
    progress_total integer NOT NULL DEFAULT 0,
    log text NOT NULL DEFAULT '',
    errors jsonb NOT NULL DEFAULT '[]',
    created_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz
);

CREATE INDEX job_runs_type_created ON job_runs (type_key, created_at DESC);

CREATE TABLE proposals (
    id uuid PRIMARY KEY,
    title text NOT NULL,
    source text NOT NULL,
    status text NOT NULL CHECK (status IN ('open', 'accepted', 'discarded')),
    actor uuid NOT NULL REFERENCES principals (id),
    on_behalf_of uuid REFERENCES principals (id),
    conflict boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE proposal_rows (
    id uuid PRIMARY KEY,
    proposal_id uuid NOT NULL REFERENCES proposals (id) ON DELETE CASCADE,
    object_id uuid,
    object_key text,
    type_key text,
    base_version bigint,
    patch jsonb NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'accepted', 'rejected')),
    conflict boolean NOT NULL DEFAULT false,
    position integer NOT NULL DEFAULT 0
);

CREATE INDEX proposal_rows_proposal ON proposal_rows (proposal_id, position);

CREATE TABLE relations (
    from_id uuid NOT NULL REFERENCES objects (id),
    to_id uuid NOT NULL REFERENCES objects (id),
    kind text NOT NULL CHECK (kind IN ('related', 'variant_of', 'article_of')),
    PRIMARY KEY (from_id, to_id, kind)
);

CREATE INDEX relations_to ON relations (to_id, kind);

CREATE TABLE webhooks (
    id uuid PRIMARY KEY,
    url text NOT NULL,
    secret text NOT NULL,
    assortment_id uuid REFERENCES assortments (id),
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE indexer_progress (
    id integer PRIMARY KEY CHECK (id = 1),
    seq bigint NOT NULL,
    collection text NOT NULL DEFAULT '',
    updated_at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO indexer_progress (id, seq) VALUES (1, 0);
