-- Relations become link objects. object_ref attributes are converted when the
-- server starts (upgrade_object_refs): that rewrite reads the definition
-- document and the stored values, which this migration does not.
--
-- A link type is added only for a kind that still has a row. Self-links are
-- dropped because a link cannot point at itself. The old table then goes away.

CREATE OR REPLACE FUNCTION braian_md5_uuid(token text)
RETURNS uuid
LANGUAGE plpgsql
IMMUTABLE
AS $$
DECLARE
    bytes bytea;
BEGIN
    bytes := decode(md5(token), 'hex');
    bytes := set_byte(bytes, 6, (get_byte(bytes, 6) & 15) | 48);
    bytes := set_byte(bytes, 8, (get_byte(bytes, 8) & 63) | 128);
    RETURN encode(bytes, 'hex')::uuid;
END;
$$;

CREATE OR REPLACE FUNCTION braian_relation_type_id(kind text)
RETURNS uuid
LANGUAGE sql
IMMUTABLE
AS $$
    SELECT CASE kind
        WHEN 'article_of' THEN '018f0000-0000-7000-8000-0000000002b1'::uuid
        WHEN 'variant_of' THEN '018f0000-0000-7000-8000-0000000002b2'::uuid
        WHEN 'related' THEN '018f0000-0000-7000-8000-0000000002b3'::uuid
    END
$$;

DO $$
DECLARE
    actor uuid;
    batch uuid;
    defs jsonb;
    revision bigint;
    next_revision bigint;
    rel_kind text;
    type_id uuid;
    type_key text;
    rel_label text;
    rel_plural text;
    from_types jsonb;
    to_types jsonb;
    key_attr text;
    attrs jsonb;
    labels jsonb;
    lang text;
    type_row jsonb;
    added boolean := false;
    link_count bigint;
    type_ids jsonb := '{}'::jsonb;
    suffix integer;
BEGIN
    SELECT count(*) INTO link_count FROM relations WHERE from_id <> to_id;
    IF link_count = 0 THEN
        RETURN;
    END IF;

    INSERT INTO principals (id, kind, name, handle)
    VALUES (braian_md5_uuid('principal:system'), 'human', 'System', 'system')
    ON CONFLICT (handle) DO UPDATE SET name = EXCLUDED.name
    RETURNING id INTO actor;

    SELECT definitions, model_revision
    INTO defs, revision
    FROM definition_revisions
    ORDER BY model_revision DESC
    LIMIT 1
    FOR UPDATE;

    IF defs IS NULL THEN
        defs := jsonb_build_object(
            'model_revision', 0,
            'languages', jsonb_build_array('en'),
            'unit_families', '[]'::jsonb,
            'attributes', '[]'::jsonb,
            'types', '[]'::jsonb,
            'structures', '[]'::jsonb
        );
        revision := 0;
    END IF;

    FOR rel_kind, rel_label, rel_plural IN
        SELECT k, l, p FROM (VALUES
            ('article_of', 'Article of', 'Articles of'),
            ('variant_of', 'Variant of', 'Variants of'),
            ('related', 'Related', 'Related')
        ) AS kinds(k, l, p)
    LOOP
        IF NOT EXISTS (
            SELECT 1 FROM relations WHERE relations.kind = rel_kind AND from_id <> to_id
        ) THEN
            CONTINUE;
        END IF;

        type_id := NULL;
        SELECT element->>'id'
        INTO type_id
        FROM jsonb_array_elements(COALESCE(defs->'types', '[]'::jsonb)) element
        WHERE lower(element->>'key') = rel_kind
          AND element->>'kind' = 'link'
        LIMIT 1;

        IF type_id IS NULL THEN
            type_id := braian_relation_type_id(rel_kind);
            type_key := rel_kind;
            -- An ordinary type may already use article_of, variant_of or related.
            IF EXISTS (
                SELECT 1
                FROM jsonb_array_elements(COALESCE(defs->'types', '[]'::jsonb)) element
                WHERE lower(element->>'key') = type_key
            ) THEN
                type_key := rel_kind || '_link';
                suffix := 2;
                WHILE EXISTS (
                    SELECT 1
                    FROM jsonb_array_elements(COALESCE(defs->'types', '[]'::jsonb)) element
                    WHERE lower(element->>'key') = type_key
                ) LOOP
                    type_key := rel_kind || '_link_' || suffix::text;
                    suffix := suffix + 1;
                END LOOP;
            END IF;

            SELECT COALESCE(jsonb_agg(DISTINCT source.type_id::text), '[]'::jsonb)
            INTO from_types
            FROM relations
            JOIN objects source ON source.id = relations.from_id
            WHERE relations.kind = rel_kind AND relations.from_id <> relations.to_id;

            SELECT COALESCE(jsonb_agg(DISTINCT target.type_id::text), '[]'::jsonb)
            INTO to_types
            FROM relations
            JOIN objects target ON target.id = relations.to_id
            WHERE relations.kind = rel_kind AND relations.from_id <> relations.to_id;

            SELECT element->>'id'
            INTO key_attr
            FROM jsonb_array_elements(COALESCE(defs->'attributes', '[]'::jsonb)) element
            WHERE element->>'key' = 'created_at'
            LIMIT 1;
            IF key_attr IS NULL THEN
                key_attr := '018f0000-0000-7000-8000-00000000a003';
            END IF;

            attrs := '[]'::jsonb;
            IF EXISTS (
                SELECT 1 FROM jsonb_array_elements(COALESCE(defs->'attributes', '[]'::jsonb)) element
                WHERE element->>'id' = '018f0000-0000-7000-8000-00000000a003'
            ) THEN
                attrs := attrs || jsonb_build_array('018f0000-0000-7000-8000-00000000a003');
            END IF;
            IF EXISTS (
                SELECT 1 FROM jsonb_array_elements(COALESCE(defs->'attributes', '[]'::jsonb)) element
                WHERE element->>'id' = '018f0000-0000-7000-8000-00000000a004'
            ) THEN
                attrs := attrs || jsonb_build_array('018f0000-0000-7000-8000-00000000a004');
            END IF;
            IF EXISTS (
                SELECT 1 FROM jsonb_array_elements(COALESCE(defs->'attributes', '[]'::jsonb)) element
                WHERE element->>'id' = '018f0000-0000-7000-8000-00000000a005'
            ) THEN
                attrs := attrs || jsonb_build_array('018f0000-0000-7000-8000-00000000a005');
            END IF;
            IF jsonb_array_length(attrs) = 0 THEN
                attrs := jsonb_build_array(key_attr);
            END IF;

            labels := '{}'::jsonb;
            FOR lang IN SELECT jsonb_array_elements_text(COALESCE(defs->'languages', '["en"]'::jsonb))
            LOOP
                labels := labels || jsonb_build_object(lang, rel_label);
            END LOOP;
            IF labels = '{}'::jsonb THEN
                labels := jsonb_build_object('en', rel_label);
            END IF;

            type_row := jsonb_build_object(
                'id', type_id,
                'key', type_key,
                'labels', labels,
                'singular', labels,
                'plural', (
                    SELECT COALESCE(jsonb_object_agg(entry.key, rel_plural), '{}'::jsonb)
                    FROM jsonb_each_text(labels) AS entry(key, value)
                ),
                'key_attribute', key_attr,
                'attributes', attrs,
                'structures', '[]'::jsonb,
                'kind', 'link',
                'link', jsonb_build_object(
                    'from_types', from_types,
                    'to_types', to_types,
                    'from_label', jsonb_build_object(
                        'key', rel_kind || '_by',
                        'labels', (
                            SELECT COALESCE(jsonb_object_agg(entry.key, rel_label || ' by'), '{}'::jsonb)
                            FROM jsonb_each_text(labels) AS entry(key, value)
                        )
                    ),
                    'to_label', jsonb_build_object(
                        'key', rel_kind,
                        'labels', labels
                    ),
                    'from_cardinality', 'many',
                    'to_cardinality', 'many'
                )
            );
            defs := jsonb_set(
                defs,
                '{types}',
                COALESCE(defs->'types', '[]'::jsonb) || jsonb_build_array(type_row)
            );
            added := true;
        END IF;
        type_ids := type_ids || jsonb_build_object(rel_kind, type_id);
    END LOOP;

    next_revision := COALESCE(revision, 0) + 1;
    batch := braian_md5_uuid('batch:relations-to-links');
    INSERT INTO batches (id, actor, kind, source)
    VALUES (batch, actor, 'bulk', 'relations-to-links');
    IF added THEN
        defs := jsonb_set(defs, '{model_revision}', to_jsonb(next_revision));
        INSERT INTO definition_revisions (model_revision, definitions, batch_id)
        VALUES (next_revision, defs, batch);
    ELSE
        next_revision := COALESCE(revision, 1);
    END IF;

    INSERT INTO objects (id, type_id, key, version, model_revision, updated_by)
    SELECT braian_md5_uuid('relation:' || relations.kind || ':' || relations.from_id::text || ':' || relations.to_id::text),
           (type_ids->>relations.kind)::uuid,
           NULL,
           1,
           next_revision,
           actor
    FROM relations
    WHERE relations.from_id <> relations.to_id;

    INSERT INTO links (object_id, link_type_id, from_id, to_id, from_one, to_one)
    SELECT object.id,
           object.type_id,
           relations.from_id,
           relations.to_id,
           COALESCE((
               SELECT (element->'link'->>'from_cardinality') = 'one'
               FROM jsonb_array_elements(COALESCE(defs->'types', '[]'::jsonb)) element
               WHERE element->>'id' = object.type_id::text
               LIMIT 1
           ), false),
           COALESCE((
               SELECT (element->'link'->>'to_cardinality') = 'one'
               FROM jsonb_array_elements(COALESCE(defs->'types', '[]'::jsonb)) element
               WHERE element->>'id' = object.type_id::text
               LIMIT 1
           ), false)
    FROM relations
    JOIN objects object ON object.id = braian_md5_uuid(
        'relation:' || relations.kind || ':' || relations.from_id::text || ':' || relations.to_id::text
    )
    WHERE relations.from_id <> relations.to_id;

    -- The same create row the API writes, so history can open the link.
    -- Attribute values stay empty, which is also what an API link with no
    -- values stores: the create diff is a marker, not a copy of the values.
    INSERT INTO changes (batch_id, object_id, version, op, actor, diff, kind)
    SELECT batch,
           object.id,
           1,
           'create',
           actor,
           '{"$created":{"version":1}}'::jsonb,
           'bulk'
    FROM relations
    JOIN objects object ON object.id = braian_md5_uuid(
        'relation:' || relations.kind || ':' || relations.from_id::text || ':' || relations.to_id::text
    )
    WHERE relations.from_id <> relations.to_id;
END;
$$;

DROP TABLE relations;
DROP FUNCTION braian_md5_uuid(text);
DROP FUNCTION braian_relation_type_id(text);
