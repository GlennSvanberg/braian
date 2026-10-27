-- A selection belongs to one type. The type is a column, not a field of `query`,
-- so saving a selection cannot point it at another type.
--
-- Rows whose query already names a type (key or id) keep that type.
-- Rows with no type are listed with RAISE NOTICE and assigned to the type
-- whose live objects match the query most often. Ties break on the type key.
-- When the query matches nothing, the type with the most live objects is used.
-- When the database has no objects, the first type key in the model is used.

ALTER TABLE assortments ADD COLUMN type_id uuid;

WITH latest AS (
    SELECT definitions
    FROM definition_revisions
    ORDER BY model_revision DESC
    LIMIT 1
),
types AS (
    SELECT elem->>'id' AS id, elem->>'key' AS key
    FROM latest
    CROSS JOIN LATERAL jsonb_array_elements(COALESCE(latest.definitions->'types', '[]'::jsonb)) AS elem
)
UPDATE assortments AS assortment
SET type_id = types.id::uuid
FROM types
WHERE assortment.type_id IS NULL
  AND jsonb_exists(assortment.query, 'type')
  AND assortment.query->>'type' <> ''
  AND (
      assortment.query->>'type' = types.id
      OR assortment.query->>'type' = types.key
  );

CREATE FUNCTION braian_selection_hit_counts(q jsonb)
RETURNS TABLE(type_id uuid, hits bigint)
LANGUAGE sql
STABLE
AS $$
WITH labels AS (
    SELECT CASE
        WHEN jsonb_exists(q, 'labels') AND jsonb_typeof(q->'labels') = 'string' AND q->>'labels' <> ''
            THEN jsonb_build_array(q->>'labels')
        WHEN jsonb_exists(q, 'labels') AND jsonb_typeof(q->'labels') = 'array'
            THEN q->'labels'
        ELSE '[]'::jsonb
    END AS codes
),
group_ref AS (
    SELECT
        CASE
            WHEN COALESCE(q->>'group', '') = '' THEN NULL
            WHEN position(':' IN q->>'group') > 0 THEN split_part(q->>'group', ':', 1)
            ELSE NULL
        END AS structure_key,
        CASE
            WHEN COALESCE(q->>'group', '') = '' THEN NULL
            WHEN position(':' IN q->>'group') > 0 THEN split_part(q->>'group', ':', 2)
            ELSE q->>'group'
        END AS group_key
),
resolved AS (
    SELECT (structure.elem->>'id')::uuid AS structure_id, (grp.elem->>'id')::uuid AS group_id
    FROM (
        SELECT definitions
        FROM definition_revisions
        ORDER BY model_revision DESC
        LIMIT 1
    ) AS latest
    CROSS JOIN group_ref
    CROSS JOIN LATERAL jsonb_array_elements(COALESCE(latest.definitions->'structures', '[]'::jsonb)) AS structure(elem)
    CROSS JOIN LATERAL jsonb_array_elements(COALESCE(structure.elem->'groups', '[]'::jsonb)) AS grp(elem)
    WHERE group_ref.group_key IS NOT NULL
      AND grp.elem->>'key' = group_ref.group_key
      AND (group_ref.structure_key IS NULL OR structure.elem->>'key' = group_ref.structure_key)
    LIMIT 1
),
wanted AS (
    SELECT
        group_ref.group_key IS NOT NULL AS needs_group,
        EXISTS (SELECT 1 FROM resolved) AS group_found,
        (SELECT structure_id FROM resolved) AS structure_id,
        (SELECT group_id FROM resolved) AS group_id,
        (SELECT codes FROM labels) AS codes
    FROM group_ref
)
SELECT objects.type_id, COUNT(*)::bigint AS hits
FROM objects
CROSS JOIN wanted
WHERE objects.deleted_at IS NULL
  AND (
      NOT wanted.needs_group
      OR (
          wanted.group_found
          AND EXISTS (
              SELECT 1
              FROM object_structures
              WHERE object_structures.object_id = objects.id
                AND object_structures.structure_id = wanted.structure_id
                AND object_structures.group_id = wanted.group_id
          )
      )
  )
  AND NOT EXISTS (
      SELECT 1
      FROM jsonb_array_elements_text(wanted.codes) AS label(code)
      WHERE NOT EXISTS (
          SELECT 1
          FROM object_values
          CROSS JOIN LATERAL jsonb_each(object_values."values") AS entry(key, value)
          WHERE object_values.object_id = objects.id
            AND object_values.is_latest
            AND jsonb_typeof(object_values."values") = 'object'
            AND (
                entry.value = to_jsonb(label.code)
                OR (jsonb_typeof(entry.value) = 'array' AND jsonb_exists(entry.value, label.code))
            )
      )
  )
GROUP BY objects.type_id
$$;

DO $$
DECLARE
    rec record;
    chosen uuid;
    chosen_key text;
    hit_count bigint;
    reason text;
BEGIN
    FOR rec IN
        SELECT id, name, query
        FROM assortments
        WHERE type_id IS NULL
        ORDER BY name, id
    LOOP
        chosen := NULL;
        chosen_key := NULL;
        hit_count := NULL;
        reason := 'most hits';

        SELECT counts.type_id, counts.hits, types.key
        INTO chosen, hit_count, chosen_key
        FROM braian_selection_hit_counts(rec.query) AS counts
        JOIN (
            SELECT elem->>'id' AS id, elem->>'key' AS key
            FROM (
                SELECT definitions
                FROM definition_revisions
                ORDER BY model_revision DESC
                LIMIT 1
            ) AS latest
            CROSS JOIN LATERAL jsonb_array_elements(COALESCE(latest.definitions->'types', '[]'::jsonb)) AS elem
        ) AS types ON types.id = counts.type_id::text
        ORDER BY counts.hits DESC, types.key, counts.type_id
        LIMIT 1;

        IF chosen IS NULL OR COALESCE(hit_count, 0) = 0 THEN
            reason := 'no query hits; type with the most objects';
            hit_count := NULL;
            SELECT objects.type_id, COUNT(*)::bigint, types.key
            INTO chosen, hit_count, chosen_key
            FROM objects
            JOIN (
                SELECT elem->>'id' AS id, elem->>'key' AS key
                FROM (
                    SELECT definitions
                    FROM definition_revisions
                    ORDER BY model_revision DESC
                    LIMIT 1
                ) AS latest
                CROSS JOIN LATERAL jsonb_array_elements(COALESCE(latest.definitions->'types', '[]'::jsonb)) AS elem
            ) AS types ON types.id = objects.type_id::text
            WHERE objects.deleted_at IS NULL
            GROUP BY objects.type_id, types.key
            ORDER BY COUNT(*) DESC, types.key, objects.type_id
            LIMIT 1;
        END IF;

        IF chosen IS NULL THEN
            reason := 'no objects; first type in the model';
            hit_count := 0;
            SELECT (elem->>'id')::uuid, elem->>'key'
            INTO chosen, chosen_key
            FROM (
                SELECT definitions
                FROM definition_revisions
                ORDER BY model_revision DESC
                LIMIT 1
            ) AS latest
            CROSS JOIN LATERAL jsonb_array_elements(COALESCE(latest.definitions->'types', '[]'::jsonb)) AS elem
            ORDER BY elem->>'key', elem->>'id'
            LIMIT 1;
        END IF;

        IF chosen IS NULL THEN
            RAISE EXCEPTION
                'selection % (%) has no type and the model has no types',
                rec.name, rec.id;
        END IF;

        RAISE NOTICE
            'selection without a type: id=% name=% assigned=% (%) reason=% hits=%',
            rec.id, rec.name, chosen_key, chosen, reason, COALESCE(hit_count, 0);

        UPDATE assortments SET type_id = chosen WHERE id = rec.id;
    END LOOP;
END $$;

UPDATE assortments
SET query = query - 'type'
WHERE jsonb_exists(query, 'type');

ALTER TABLE assortments ALTER COLUMN type_id SET NOT NULL;

CREATE INDEX assortments_type_id ON assortments (type_id);

UPDATE view_types
SET title = 'Selections'
WHERE key = 'assortment_list';

UPDATE views
SET title = 'Selections'
WHERE type_key = 'assortment_list'
  AND title IN ('Assortments', 'Sortiment');

DROP FUNCTION braian_selection_hit_counts(jsonb);
