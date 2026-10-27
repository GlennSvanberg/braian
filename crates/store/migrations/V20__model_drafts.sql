-- One unsaved model draft per person. Steps are ModelChange values.
-- base_revision is the model revision the steps were written against.

CREATE TABLE model_drafts (
    user_id uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    base_revision bigint NOT NULL,
    steps jsonb NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);
