-- Stable storage groups. An attribute keeps the group it was first given, so
-- adding an attribute does not move existing values between parts.

CREATE TABLE attribute_parts (
    attribute_id uuid PRIMARY KEY,
    part_group integer NOT NULL CHECK (part_group >= 0)
);

-- Dev tokens look principals up by handle. Rows created before this column stay NULL.
ALTER TABLE principals ADD COLUMN handle text UNIQUE;
