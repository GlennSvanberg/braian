CREATE TABLE workspace_states (
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    environment text NOT NULL,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    document jsonb NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, environment, workspace_id)
);
