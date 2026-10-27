-- Lease for an accept or discard that is still running.
-- claimed_by is a worker token, not a principal. A crash leaves the row
-- claimed until claimed_until, then another worker may take it. Startup
-- clears leases that are already due. The work itself does not hold a
-- pooled connection.

ALTER TABLE proposals
    ADD COLUMN claimed_by uuid,
    ADD COLUMN claimed_until timestamptz;
