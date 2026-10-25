CREATE TABLE command_frecency (
    principal_id uuid NOT NULL REFERENCES principals (id) ON DELETE CASCADE,
    command_id text NOT NULL,
    uses integer NOT NULL,
    last_at timestamptz NOT NULL,
    PRIMARY KEY (principal_id, command_id)
);
