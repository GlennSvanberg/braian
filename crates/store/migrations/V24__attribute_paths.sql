-- Saved filters, sorts and import targets address an attribute path.
-- A path is `<attribute id>`, `<id>.unit`, `<id>.min` or `<id>@language`.
-- Group, membership and link-endpoint targets stay. Column ids stay strings.
-- New index fields (`a_<id>__unit`, `__min`, `__max`) need a rebuild.

CREATE OR REPLACE FUNCTION braian_uuid_text(token text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
AS $$
    SELECT token ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
$$;

CREATE OR REPLACE FUNCTION braian_attr_id(defs jsonb, token text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    element jsonb;
    dashed text;
BEGIN
    IF token IS NULL OR token = '' THEN
        RETURN NULL;
    END IF;
    dashed := token;
    IF token ~* '^[0-9a-f]{8}_[0-9a-f]{4}_[0-9a-f]{4}_[0-9a-f]{4}_[0-9a-f]{12}$' THEN
        dashed := replace(token, '_', '-');
    END IF;
    IF braian_uuid_text(dashed) THEN
        FOR element IN SELECT value FROM jsonb_array_elements(COALESCE(defs->'attributes', '[]'::jsonb))
        LOOP
            IF element->>'id' = dashed THEN
                RETURN dashed;
            END IF;
        END LOOP;
        RETURN NULL;
    END IF;
    FOR element IN SELECT value FROM jsonb_array_elements(COALESCE(defs->'attributes', '[]'::jsonb))
    LOOP
        IF lower(element->>'key') = lower(token) THEN
            RETURN element->>'id';
        END IF;
    END LOOP;
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION braian_type_id(defs jsonb, token text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    element jsonb;
BEGIN
    IF token IS NULL OR token = '' OR braian_uuid_text(token) THEN
        RETURN token;
    END IF;
    FOR element IN SELECT value FROM jsonb_array_elements(COALESCE(defs->'types', '[]'::jsonb))
    LOOP
        IF lower(element->>'key') = lower(token) THEN
            RETURN element->>'id';
        END IF;
    END LOOP;
    RETURN token;
END;
$$;

CREATE OR REPLACE FUNCTION braian_structure_id(defs jsonb, token text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    element jsonb;
BEGIN
    IF token IS NULL OR token = '' OR braian_uuid_text(token) THEN
        RETURN token;
    END IF;
    FOR element IN SELECT value FROM jsonb_array_elements(COALESCE(defs->'structures', '[]'::jsonb))
    LOOP
        IF lower(element->>'key') = lower(token) THEN
            RETURN element->>'id';
        END IF;
    END LOOP;
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
        RETURN token;
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

CREATE OR REPLACE FUNCTION braian_path_field(defs jsonb, field text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    work text;
    suffix text;
    part text := '';
    language text := '';
    attribute_id text;
    result text;
BEGIN
    IF field IS NULL OR btrim(field) = '' THEN
        RETURN field;
    END IF;
    IF field IN ('key', 'type_key', 'groups', 'version', 'seq', 'deleted', 'id')
       OR left(field, 7) = 'search_'
       OR position('>' in field) > 0
    THEN
        RETURN field;
    END IF;
    -- A key is resolved whole before any suffix is split off. `size__min` and
    -- `name__en` are attribute keys, not the min part or the English value.
    attribute_id := braian_attr_id(defs, field);
    IF attribute_id IS NOT NULL THEN
        RETURN attribute_id;
    END IF;
    work := field;
    IF left(work, 2) = 'a_' THEN
        work := substring(work from 3);
        attribute_id := braian_attr_id(defs, work);
        IF attribute_id IS NOT NULL THEN
            RETURN attribute_id;
        END IF;
    END IF;
    LOOP
        EXIT WHEN position('__' in work) = 0;
        suffix := reverse(split_part(reverse(work), '__', 1));
        IF suffix IN ('unit', 'min', 'max') THEN
            IF part = '' THEN
                part := suffix;
            END IF;
        ELSIF suffix ~ '^[A-Za-z0-9_]{1,16}$' THEN
            IF language = '' THEN
                language := replace(suffix, '_', '-');
            END IF;
        ELSE
            EXIT;
        END IF;
        work := left(work, length(work) - length(suffix) - 2);
        -- `name__en__sv` keeps the key `name__en` and the language `sv`.
        attribute_id := braian_attr_id(defs, work);
        EXIT WHEN attribute_id IS NOT NULL;
    END LOOP;
    IF position('@' in work) > 0 THEN
        language := split_part(work, '@', 2);
        work := split_part(work, '@', 1);
    END IF;
    IF position('.' in work) > 0 THEN
        IF part = '' AND split_part(work, '.', 2) IN ('value', 'unit', 'min', 'max', 'code', 'label') THEN
            part := split_part(work, '.', 2);
        END IF;
        work := split_part(work, '.', 1);
    END IF;
    attribute_id := braian_attr_id(defs, work);
    IF attribute_id IS NULL THEN
        RETURN field;
    END IF;
    result := attribute_id;
    IF part <> '' AND part <> 'value' AND part <> 'code' THEN
        result := result || '.' || part;
    ELSIF part IN ('value', 'code') THEN
        result := result || '.' || part;
    END IF;
    IF language <> '' THEN
        result := result || '@' || language;
    END IF;
    RETURN result;
END;
$$;

CREATE OR REPLACE FUNCTION braian_path_clause(defs jsonb, clause text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    work text := btrim(clause);
    operators text[] := ARRAY[':>=', ':<=', ':!=', ':=', ':>', ':<', '!=', '='];
    operator text;
    field text;
    rest text;
    at int;
BEGIN
    IF work = '' THEN
        RETURN work;
    END IF;
    FOREACH operator IN ARRAY operators
    LOOP
        at := position(operator in work);
        IF at > 0 THEN
            field := btrim(left(work, at - 1));
            rest := substring(work from at + length(operator));
            IF field = 'type_key' THEN
                RETURN 'type_key:=' || braian_type_id(defs, btrim(rest));
            ELSIF field = 'groups' THEN
                RETURN 'groups:=' || braian_group_token(defs, btrim(rest));
            ELSIF field IN ('key', 'version', 'seq', 'deleted') OR left(field, 7) = 'search_' THEN
                RETURN work;
            ELSE
                IF operator = '=' THEN
                    operator := ':=';
                END IF;
                RETURN braian_path_field(defs, field) || operator || rest;
            END IF;
        END IF;
    END LOOP;
    RETURN braian_path_field(defs, work);
END;
$$;

CREATE OR REPLACE FUNCTION braian_path_filter(defs jsonb, filter text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    part text;
    parts text[] := ARRAY[]::text[];
    normalized text;
BEGIN
    IF filter IS NULL OR btrim(filter) = '' THEN
        RETURN filter;
    END IF;
    normalized := replace(filter, ' && ', '&&');
    FOREACH part IN ARRAY string_to_array(normalized, '&&')
    LOOP
        IF btrim(part) <> '' THEN
            parts := parts || braian_path_clause(defs, part);
        END IF;
    END LOOP;
    RETURN array_to_string(parts, ' && ');
END;
$$;

CREATE OR REPLACE FUNCTION braian_path_sort(defs jsonb, token text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    field text;
    direction text := '';
BEGIN
    IF token IS NULL OR btrim(token) = '' THEN
        RETURN token;
    END IF;
    IF right(token, 4) = ':asc' THEN
        field := left(token, length(token) - 4);
        direction := ':asc';
    ELSIF right(token, 5) = ':desc' THEN
        field := left(token, length(token) - 5);
        direction := ':desc';
    ELSE
        field := token;
    END IF;
    IF field IN ('key', 'type_key', 'version', 'seq', 'deleted') THEN
        RETURN token;
    END IF;
    RETURN braian_path_field(defs, field) || direction;
END;
$$;

CREATE OR REPLACE FUNCTION braian_path_target(defs jsonb, target text)
RETURNS text
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    body text;
    language text := '';
    rest text;
    part text := '';
    attribute_token text;
    structure_token text;
    group_token text;
    link_token text;
    side text;
    attribute_id text;
    group_ref text;
BEGIN
    IF target IS NULL OR target IN ('action', 'unmapped', '') THEN
        RETURN target;
    END IF;
    IF position('@' in target) > 0 AND position(':' in split_part(target, '@', 2)) = 0 THEN
        body := split_part(target, '@', 1);
        language := split_part(target, '@', 2);
    ELSE
        body := target;
    END IF;
    IF left(body, 11) = 'membership:' THEN
        rest := 'membership:' || braian_structure_id(defs, substring(body from 12));
    ELSIF left(body, 5) = 'link:' THEN
        rest := substring(body from 6);
        side := split_part(rest, ':', 2);
        link_token := split_part(rest, ':', 1);
        IF side = '' THEN
            rest := body;
        ELSE
            rest := 'link:' || braian_type_id(defs, link_token) || ':' || side;
        END IF;
    ELSIF left(body, 6) = 'group:' THEN
        rest := substring(body from 7);
        structure_token := split_part(split_part(rest, ':', 1), '/', 1);
        group_token := split_part(split_part(rest, ':', 1), '/', 2);
        attribute_token := split_part(rest, ':', 2);
        IF position('.' in attribute_token) > 0 THEN
            part := split_part(attribute_token, '.', 2);
            attribute_token := split_part(attribute_token, '.', 1);
        END IF;
        group_ref := braian_group_token(defs, structure_token || ':' || group_token);
        attribute_id := braian_attr_id(defs, attribute_token);
        rest := 'group:' || split_part(group_ref, ':', 1) || '/' || split_part(group_ref, ':', 2) || ':' || COALESCE(attribute_id, attribute_token);
        IF part <> '' THEN
            rest := rest || '.' || part;
        END IF;
    ELSIF left(body, 5) = 'type:' THEN
        attribute_token := substring(body from 6);
        IF position('.' in attribute_token) > 0
           AND split_part(attribute_token, '.', 2) IN ('value', 'unit', 'min', 'max', 'code', 'label')
        THEN
            part := split_part(attribute_token, '.', 2);
            attribute_token := split_part(attribute_token, '.', 1);
        END IF;
        attribute_id := braian_attr_id(defs, attribute_token);
        IF attribute_id IS NULL THEN
            rest := body;
        ELSE
            rest := attribute_id;
            IF part <> '' THEN
                rest := rest || '.' || part;
            END IF;
        END IF;
    ELSE
        attribute_token := body;
        IF position('.' in attribute_token) > 0
           AND split_part(attribute_token, '.', 2) IN ('value', 'unit', 'min', 'max', 'code', 'label')
        THEN
            part := split_part(attribute_token, '.', 2);
            attribute_token := split_part(attribute_token, '.', 1);
        END IF;
        attribute_id := braian_attr_id(defs, attribute_token);
        IF attribute_id IS NULL THEN
            RETURN target;
        END IF;
        rest := attribute_id;
        IF part <> '' THEN
            rest := rest || '.' || part;
        END IF;
    END IF;
    IF language = '' THEN
        RETURN rest;
    END IF;
    RETURN rest || '@' || language;
END;
$$;

CREATE OR REPLACE FUNCTION braian_path_mapping(defs jsonb, mapping jsonb)
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
        result := jsonb_set(result, '{type}', to_jsonb(braian_type_id(defs, mapping->>'type')));
    END IF;
    IF jsonb_typeof(mapping->'columns') = 'array' THEN
        FOR column_row IN SELECT value FROM jsonb_array_elements(mapping->'columns')
        LOOP
            target := column_row->>'target';
            IF target IS NOT NULL THEN
                column_row := jsonb_set(column_row, '{target}', to_jsonb(braian_path_target(defs, target)));
            END IF;
            columns := columns || column_row;
        END LOOP;
        result := jsonb_set(result, '{columns}', columns);
    END IF;
    RETURN result;
END;
$$;

CREATE OR REPLACE FUNCTION braian_path_view(defs jsonb, state jsonb)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    result jsonb := state;
    table_state jsonb;
BEGIN
    IF state IS NULL OR jsonb_typeof(state) <> 'object' THEN
        RETURN state;
    END IF;
    -- Column ids stay the strings the table addresses. Row 51 turns them into paths.
    IF jsonb_typeof(state->'sort') = 'string' THEN
        result := jsonb_set(result, '{sort}', to_jsonb(braian_path_sort(defs, state->>'sort')));
    END IF;
    IF jsonb_typeof(state->'type') = 'string' THEN
        result := jsonb_set(result, '{type}', to_jsonb(braian_type_id(defs, state->>'type')));
    END IF;
    IF jsonb_typeof(state->'type_key') = 'string' THEN
        result := jsonb_set(result, '{type_key}', to_jsonb(braian_type_id(defs, state->>'type_key')));
    END IF;
    IF jsonb_typeof(state->'filter') = 'string' THEN
        result := jsonb_set(result, '{filter}', to_jsonb(braian_path_filter(defs, state->>'filter')));
    END IF;
    IF jsonb_typeof(state->'table') = 'object' THEN
        table_state := state->'table';
        IF jsonb_typeof(table_state->'sort') = 'string' THEN
            table_state := jsonb_set(table_state, '{sort}', to_jsonb(braian_path_sort(defs, table_state->>'sort')));
        END IF;
        IF jsonb_typeof(table_state->'groupBy') = 'string' THEN
            table_state := jsonb_set(table_state, '{groupBy}', to_jsonb(braian_path_sort(defs, table_state->>'groupBy')));
        END IF;
        IF jsonb_typeof(table_state->'filter') = 'string' THEN
            table_state := jsonb_set(table_state, '{filter}', to_jsonb(braian_path_filter(defs, table_state->>'filter')));
        END IF;
        result := jsonb_set(result, '{table}', table_state);
    END IF;
    RETURN result;
END;
$$;

CREATE OR REPLACE FUNCTION braian_path_query(defs jsonb, source jsonb)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    result jsonb := source;
BEGIN
    IF source IS NULL OR jsonb_typeof(source) <> 'object' THEN
        RETURN source;
    END IF;
    result := result - 'type';
    IF jsonb_typeof(source->'group') = 'string' THEN
        result := jsonb_set(result, '{group}', to_jsonb(braian_group_token(defs, source->>'group')));
    END IF;
    IF jsonb_typeof(source->'filter') = 'string' THEN
        result := jsonb_set(result, '{filter}', to_jsonb(braian_path_filter(defs, source->>'filter')));
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
    SET state = braian_path_view(defs, state);
    UPDATE assortments
    SET query = braian_path_query(defs, query);
    UPDATE import_sessions
    SET mapping = braian_path_mapping(defs, mapping)
    WHERE mapping IS NOT NULL;
    UPDATE import_sessions
    SET diff_mapping = braian_path_mapping(defs, diff_mapping)
    WHERE diff_mapping IS NOT NULL;
END;
$$;

UPDATE indexer_progress
SET reindex_requested = true, updated_at = now()
WHERE id = 1;

DROP FUNCTION braian_path_query(jsonb, jsonb);
DROP FUNCTION braian_path_view(jsonb, jsonb);
DROP FUNCTION braian_path_mapping(jsonb, jsonb);
DROP FUNCTION braian_path_target(jsonb, text);
DROP FUNCTION braian_path_sort(jsonb, text);
DROP FUNCTION braian_path_filter(jsonb, text);
DROP FUNCTION braian_path_clause(jsonb, text);
DROP FUNCTION braian_path_field(jsonb, text);
DROP FUNCTION braian_group_token(jsonb, text);
DROP FUNCTION braian_structure_id(jsonb, text);
DROP FUNCTION braian_type_id(jsonb, text);
DROP FUNCTION braian_attr_id(jsonb, text);
DROP FUNCTION braian_uuid_text(text);
