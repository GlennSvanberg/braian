-- Stored view state, selection queries and import mappings keep ids.
-- A selection type lives in assortments.type_id; this rewrite does not put type back into query.
-- A key that is still stored is rewritten from the latest definition revision.
-- Attribute index fields change from a_<key> to a_<id>, so the indexer rebuilds.

UPDATE view_types
SET title = 'Group attributes'
WHERE key = 'class_attributes' AND title = 'Classes';

UPDATE views
SET title = 'Group attributes'
WHERE type_key = 'class_attributes' AND title = 'Classes';

CREATE OR REPLACE FUNCTION braian_uuid_text(token text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
AS $$
    SELECT token ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
$$;

CREATE OR REPLACE FUNCTION braian_ref_id(defs jsonb, kind text, token text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    found text;
    element jsonb;
BEGIN
    IF token IS NULL OR token = '' OR braian_uuid_text(token) THEN
        RETURN token;
    END IF;
    IF kind = 'attribute' THEN
        FOR element IN SELECT value FROM jsonb_array_elements(COALESCE(defs->'attributes', '[]'::jsonb))
        LOOP
            IF lower(element->>'key') = lower(token) THEN
                RETURN element->>'id';
            END IF;
        END LOOP;
    ELSIF kind = 'type' THEN
        FOR element IN SELECT value FROM jsonb_array_elements(COALESCE(defs->'types', '[]'::jsonb))
        LOOP
            IF lower(element->>'key') = lower(token) THEN
                RETURN element->>'id';
            END IF;
        END LOOP;
    ELSIF kind = 'structure' THEN
        FOR element IN SELECT value FROM jsonb_array_elements(COALESCE(defs->'structures', '[]'::jsonb))
        LOOP
            IF lower(element->>'key') = lower(token) THEN
                RETURN element->>'id';
            END IF;
        END LOOP;
    ELSIF kind = 'group' THEN
        FOR element IN SELECT value FROM jsonb_array_elements(COALESCE(defs->'structures', '[]'::jsonb))
        LOOP
            FOR found IN
                SELECT group_row->>'id'
                FROM jsonb_array_elements(COALESCE(element->'groups', '[]'::jsonb)) group_row
                WHERE lower(group_row->>'key') = lower(token)
            LOOP
                RETURN found;
            END LOOP;
        END LOOP;
    END IF;
    RETURN token;
END;
$$;

CREATE OR REPLACE FUNCTION braian_group_token(defs jsonb, token text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    structure_token text;
    group_token text;
    structure_row jsonb;
    group_row jsonb;
BEGIN
    IF token IS NULL OR token = '' OR position(':' in token) = 0 THEN
        RETURN braian_ref_id(defs, 'group', token);
    END IF;
    structure_token := split_part(token, ':', 1);
    group_token := split_part(token, ':', 2);
    IF braian_uuid_text(structure_token) AND braian_uuid_text(group_token) THEN
        RETURN token;
    END IF;
    FOR structure_row IN SELECT value FROM jsonb_array_elements(COALESCE(defs->'structures', '[]'::jsonb))
    LOOP
        IF lower(structure_row->>'key') = lower(structure_token)
           OR structure_row->>'id' = structure_token
        THEN
            FOR group_row IN SELECT value FROM jsonb_array_elements(COALESCE(structure_row->'groups', '[]'::jsonb))
            LOOP
                IF lower(group_row->>'key') = lower(group_token) OR group_row->>'id' = group_token THEN
                    RETURN (structure_row->>'id') || ':' || (group_row->>'id');
                END IF;
            END LOOP;
        END IF;
    END LOOP;
    RETURN token;
END;
$$;

CREATE OR REPLACE FUNCTION braian_index_field(defs jsonb, field text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    body text;
    language text;
    token text;
    attribute_id text;
BEGIN
    IF field IS NULL OR field = '' OR field IN ('key', 'type_key', 'version', 'seq', 'deleted') THEN
        RETURN field;
    END IF;
    IF position('__' in field) > 0 THEN
        body := split_part(field, '__', 1);
        language := split_part(field, '__', 2);
    ELSE
        body := field;
        language := '';
    END IF;
    IF left(body, 2) = 'a_' THEN
        token := substring(body from 3);
    ELSE
        token := body;
    END IF;
    attribute_id := braian_ref_id(defs, 'attribute', replace(token, '_', '-'));
    IF attribute_id = replace(token, '_', '-') AND NOT braian_uuid_text(attribute_id) THEN
        attribute_id := braian_ref_id(defs, 'attribute', token);
    END IF;
    IF attribute_id IS NULL OR attribute_id = token OR NOT braian_uuid_text(attribute_id) THEN
        RETURN field;
    END IF;
    IF language = '' THEN
        RETURN 'a_' || replace(attribute_id, '-', '_');
    END IF;
    RETURN 'a_' || replace(attribute_id, '-', '_') || '__' || replace(language, '-', '_');
END;
$$;

CREATE OR REPLACE FUNCTION braian_translate_filter(defs jsonb, filter text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    part text;
    parts text[] := ARRAY[]::text[];
    field text;
    rest text;
BEGIN
    IF filter IS NULL OR filter = '' THEN
        RETURN filter;
    END IF;
    FOREACH part IN ARRAY string_to_array(filter, ' && ')
    LOOP
        IF position(':=' in part) = 0 THEN
            parts := parts || part;
        ELSE
            field := split_part(part, ':=', 1);
            rest := substring(part from position(':=' in part) + 2);
            IF field = 'type_key' THEN
                parts := parts || ('type_key:=' || braian_ref_id(defs, 'type', rest));
            ELSIF field = 'groups' THEN
                parts := parts || ('groups:=' || braian_group_token(defs, rest));
            ELSIF left(field, 2) = 'a_' THEN
                parts := parts || (braian_index_field(defs, field) || ':=' || rest);
            ELSE
                parts := parts || part;
            END IF;
        END IF;
    END LOOP;
    RETURN array_to_string(parts, ' && ');
END;
$$;

CREATE OR REPLACE FUNCTION braian_translate_column(defs jsonb, token text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
BEGIN
    IF token IN ('key', 'type', 'version') THEN
        RETURN token;
    END IF;
    RETURN braian_ref_id(defs, 'attribute', token);
END;
$$;

CREATE OR REPLACE FUNCTION braian_translate_view_state(defs jsonb, state jsonb)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    result jsonb := state;
    columns jsonb := '[]'::jsonb;
    item jsonb;
    text_value text;
    expanded jsonb := '[]'::jsonb;
    table_state jsonb;
    order_columns jsonb := '[]'::jsonb;
BEGIN
    IF state IS NULL OR jsonb_typeof(state) <> 'object' THEN
        RETURN state;
    END IF;
    IF jsonb_typeof(state->'columns') = 'array' THEN
        FOR item IN SELECT value FROM jsonb_array_elements(state->'columns')
        LOOP
            IF jsonb_typeof(item) = 'string' THEN
                columns := columns || to_jsonb(braian_translate_column(defs, item #>> '{}'));
            ELSE
                columns := columns || item;
            END IF;
        END LOOP;
        result := jsonb_set(result, '{columns}', columns);
    END IF;
    IF jsonb_typeof(state->'sort') = 'string' THEN
        result := jsonb_set(result, '{sort}', to_jsonb(braian_ref_id(defs, 'attribute', state->>'sort')));
    END IF;
    IF jsonb_typeof(state->'type') = 'string' THEN
        result := jsonb_set(result, '{type}', to_jsonb(braian_ref_id(defs, 'type', state->>'type')));
    END IF;
    IF jsonb_typeof(state->'type_key') = 'string' THEN
        result := jsonb_set(result, '{type_key}', to_jsonb(braian_ref_id(defs, 'type', state->>'type_key')));
    END IF;
    IF jsonb_typeof(state->'expanded') = 'array' THEN
        FOR item IN SELECT value FROM jsonb_array_elements(state->'expanded')
        LOOP
            IF jsonb_typeof(item) = 'string' THEN
                expanded := expanded || to_jsonb(braian_group_token(defs, item #>> '{}'));
            ELSE
                expanded := expanded || item;
            END IF;
        END LOOP;
        result := jsonb_set(result, '{expanded}', expanded);
    END IF;
    IF jsonb_typeof(state->'table') = 'object' THEN
        table_state := state->'table';
        IF jsonb_typeof(table_state->'sort') = 'string' THEN
            text_value := table_state->>'sort';
            IF right(text_value, 4) = ':asc' THEN
                table_state := jsonb_set(
                    table_state,
                    '{sort}',
                    to_jsonb(braian_index_field(defs, left(text_value, length(text_value) - 4)) || ':asc')
                );
            ELSIF right(text_value, 5) = ':desc' THEN
                table_state := jsonb_set(
                    table_state,
                    '{sort}',
                    to_jsonb(braian_index_field(defs, left(text_value, length(text_value) - 5)) || ':desc')
                );
            ELSE
                table_state := jsonb_set(table_state, '{sort}', to_jsonb(braian_index_field(defs, text_value)));
            END IF;
        END IF;
        IF jsonb_typeof(table_state->'groupBy') = 'string' THEN
            table_state := jsonb_set(table_state, '{groupBy}', to_jsonb(braian_index_field(defs, table_state->>'groupBy')));
        END IF;
        IF jsonb_typeof(table_state->'filter') = 'string' THEN
            table_state := jsonb_set(table_state, '{filter}', to_jsonb(braian_translate_filter(defs, table_state->>'filter')));
        END IF;
        IF jsonb_typeof(table_state->'order') = 'array' THEN
            FOR item IN SELECT value FROM jsonb_array_elements(table_state->'order')
            LOOP
            IF jsonb_typeof(item) = 'string' THEN
                text_value := item #>> '{}';
                -- Object-table column ids. name and status are also attribute keys.
                IF text_value IN ('key', 'name', 'type', 'status', 'version') THEN
                    order_columns := order_columns || item;
                ELSE
                    order_columns := order_columns || to_jsonb(braian_translate_column(defs, text_value));
                END IF;
            ELSE
                order_columns := order_columns || item;
            END IF;
            END LOOP;
            table_state := jsonb_set(table_state, '{order}', order_columns);
        END IF;
        result := jsonb_set(result, '{table}', table_state);
    END IF;
    RETURN result;
END;
$$;

CREATE OR REPLACE FUNCTION braian_translate_query(defs jsonb, source jsonb)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    result jsonb := source;
BEGIN
    -- The argument is not named query: RETURN query is the set-returning statement.
    IF source IS NULL OR jsonb_typeof(source) <> 'object' THEN
        RETURN source;
    END IF;
    -- V17 stores the type in assortments.type_id and strips query.type.
    result := result - 'type';
    IF jsonb_typeof(source->'group') = 'string' THEN
        result := jsonb_set(result, '{group}', to_jsonb(braian_group_token(defs, source->>'group')));
    END IF;
    IF jsonb_typeof(source->'filter') = 'string' THEN
        result := jsonb_set(result, '{filter}', to_jsonb(braian_translate_filter(defs, source->>'filter')));
    END IF;
    RETURN result;
END;
$$;

CREATE OR REPLACE FUNCTION braian_translate_target(defs jsonb, target text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    body text;
    language text;
    rest text;
    structure_token text;
    group_token text;
    attribute_token text;
    path text;
BEGIN
    IF target IS NULL OR target IN ('action', 'unmapped', '') THEN
        RETURN target;
    END IF;
    IF position('@' in target) > 0 AND position(':' in split_part(target, '@', 2)) = 0 THEN
        body := split_part(target, '@', 1);
        language := split_part(target, '@', 2);
    ELSE
        body := target;
        language := '';
    END IF;
    IF left(body, 11) = 'membership:' THEN
        rest := 'membership:' || braian_ref_id(defs, 'structure', substring(body from 12));
    ELSIF left(body, 5) = 'type:' THEN
        rest := 'type:' || braian_ref_id(defs, 'attribute', substring(body from 6));
    ELSIF left(body, 6) = 'group:' THEN
        path := substring(body from 7);
        structure_token := split_part(split_part(path, ':', 1), '/', 1);
        group_token := split_part(split_part(path, ':', 1), '/', 2);
        attribute_token := split_part(path, ':', 2);
        rest := 'group:'
            || braian_ref_id(defs, 'structure', structure_token) || '/'
            || split_part(braian_group_token(defs, structure_token || ':' || group_token), ':', 2) || ':'
            || braian_ref_id(defs, 'attribute', attribute_token);
    ELSE
        rest := body;
    END IF;
    IF language = '' THEN
        RETURN rest;
    END IF;
    RETURN rest || '@' || language;
END;
$$;

CREATE OR REPLACE FUNCTION braian_translate_mapping(defs jsonb, mapping jsonb)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    result jsonb := mapping;
    columns jsonb := '[]'::jsonb;
    column_row jsonb;
    target text;
BEGIN
    IF mapping IS NULL OR jsonb_typeof(mapping) <> 'object' THEN
        RETURN mapping;
    END IF;
    IF jsonb_typeof(mapping->'type') = 'string' THEN
        result := jsonb_set(result, '{type}', to_jsonb(braian_ref_id(defs, 'type', mapping->>'type')));
    END IF;
    IF jsonb_typeof(mapping->'columns') = 'array' THEN
        FOR column_row IN SELECT value FROM jsonb_array_elements(mapping->'columns')
        LOOP
            target := column_row->>'target';
            IF target IS NOT NULL THEN
                column_row := jsonb_set(column_row, '{target}', to_jsonb(braian_translate_target(defs, target)));
            END IF;
            columns := columns || column_row;
        END LOOP;
        result := jsonb_set(result, '{columns}', columns);
    END IF;
    RETURN result;
END;
$$;

DO $$
DECLARE
    defs jsonb;
BEGIN
    SELECT definitions INTO defs
    FROM definition_revisions
    ORDER BY model_revision DESC
    LIMIT 1;
    IF defs IS NULL THEN
        RETURN;
    END IF;
    UPDATE views
    SET state = braian_translate_view_state(defs, state);
    UPDATE assortments
    SET query = braian_translate_query(defs, query);
    UPDATE import_sessions
    SET mapping = braian_translate_mapping(defs, mapping)
    WHERE mapping IS NOT NULL;
    UPDATE import_sessions
    SET diff_mapping = braian_translate_mapping(defs, diff_mapping)
    WHERE diff_mapping IS NOT NULL;
END;
$$;

UPDATE indexer_progress
SET reindex_requested = true, updated_at = now()
WHERE id = 1;

DROP FUNCTION braian_translate_mapping(jsonb, jsonb);
DROP FUNCTION braian_translate_target(jsonb, text);
DROP FUNCTION braian_translate_query(jsonb, jsonb);
DROP FUNCTION braian_translate_view_state(jsonb, jsonb);
DROP FUNCTION braian_translate_filter(jsonb, text);
DROP FUNCTION braian_translate_column(jsonb, text);
DROP FUNCTION braian_index_field(jsonb, text);
DROP FUNCTION braian_group_token(jsonb, text);
DROP FUNCTION braian_ref_id(jsonb, text, text);
DROP FUNCTION braian_uuid_text(text);
