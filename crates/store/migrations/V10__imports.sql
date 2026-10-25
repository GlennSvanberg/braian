CREATE TABLE import_sessions (
    id uuid PRIMARY KEY,
    owner uuid NOT NULL REFERENCES principals (id),
    on_behalf_of uuid REFERENCES principals (id),
    file_name text NOT NULL,
    file_kind text NOT NULL CHECK (file_kind IN ('csv', 'xlsx', 'xls')),
    file_bytes bigint NOT NULL DEFAULT 0,
    status text NOT NULL CHECK (status IN
        ('uploading','mapping','ready','diffing','previewed','saving','saved','failed','cancelled')),
    sheets jsonb NOT NULL DEFAULT '[]',
    sheet_index integer NOT NULL DEFAULT 0,
    csv_dialect jsonb,
    headers jsonb NOT NULL DEFAULT '[]',
    sample jsonb NOT NULL DEFAULT '[]',
    rows_parsed bigint NOT NULL DEFAULT 0,
    parse_done boolean NOT NULL DEFAULT false,
    model_revision bigint NOT NULL,
    proposal jsonb,
    mapping jsonb,
    diff_mapping jsonb,
    diff_generation bigint NOT NULL DEFAULT 0,
    counters jsonb NOT NULL DEFAULT
      '{"new":0,"changed":0,"unchanged":0,"failed":0,"delete":0,"processed":0,"total":0}',
    job_id uuid,
    error text,
    decisions_usage jsonb NOT NULL DEFAULT '[]',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX import_sessions_owner ON import_sessions (owner, created_at DESC);

CREATE UNLOGGED TABLE import_rows (
    session_id uuid NOT NULL REFERENCES import_sessions (id) ON DELETE CASCADE,
    row_no integer NOT NULL,
    status text NOT NULL CHECK (status IN ('new','changed','failed','delete')),
    object_id uuid,
    object_key text,
    base_version bigint,
    cells jsonb NOT NULL,
    patch jsonb,
    errors jsonb NOT NULL DEFAULT '[]',
    PRIMARY KEY (session_id, row_no)
);
CREATE INDEX import_rows_status ON import_rows (session_id, status, row_no);
