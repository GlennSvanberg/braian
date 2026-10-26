-- A row is claimed (`applying`) before its patch is written, so two accepts
-- cannot both apply it. `accepted` is set only after the write commits.
-- A crash can leave `applying`; that row is not applied again.

DO $$
DECLARE
    existing text;
BEGIN
    SELECT con.conname INTO existing
    FROM pg_constraint con
    JOIN pg_class rel ON rel.oid = con.conrelid
    WHERE rel.relname = 'proposal_rows'
      AND con.contype = 'c'
      AND pg_get_constraintdef(con.oid) LIKE '%pending%';
    IF existing IS NOT NULL THEN
        EXECUTE format('ALTER TABLE proposal_rows DROP CONSTRAINT %I', existing);
    END IF;
END $$;

ALTER TABLE proposal_rows
    ADD CONSTRAINT proposal_rows_status_check
    CHECK (status IN ('pending', 'applying', 'accepted', 'rejected'));
