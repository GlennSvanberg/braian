-- A link is an object. Its number (objects.key) stays null.
ALTER TABLE objects ALTER COLUMN key DROP NOT NULL;

-- from_one and to_one copy the link type's cardinality so the unique indexes
-- can reject a second link when that side is `one`.
CREATE TABLE links (
    object_id uuid PRIMARY KEY REFERENCES objects (id),
    link_type_id uuid NOT NULL,
    from_id uuid NOT NULL REFERENCES objects (id),
    to_id uuid NOT NULL REFERENCES objects (id),
    from_one boolean NOT NULL,
    to_one boolean NOT NULL,
    CHECK (from_id <> to_id)
);

CREATE INDEX links_by_from ON links (from_id, link_type_id);
CREATE INDEX links_by_to ON links (to_id, link_type_id);
CREATE UNIQUE INDEX links_one_from ON links (link_type_id, from_id) WHERE from_one;
CREATE UNIQUE INDEX links_one_to ON links (link_type_id, to_id) WHERE to_one;
