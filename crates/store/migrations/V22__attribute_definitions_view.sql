-- Attribute definitions is a model view, separate from an object's Attributes panel.
-- PR 8 owns V21. This file is V22 so the two branches do not share a version.
-- Merge it after V21, or renumber it to V21 if this branch merges first.
-- refinery rejects a later migration whose version is below one already applied.

INSERT INTO view_types (key, title, builtin)
VALUES ('attribute_definitions', 'Attribute definitions', true)
ON CONFLICT (key) DO UPDATE SET title = EXCLUDED.title;

INSERT INTO views (id, workspace_id, type_key, title, pane, position, state)
SELECT
    '018f2000-0000-7000-8000-000000000204'::uuid,
    '018f2000-0000-7000-8000-000000000002'::uuid,
    'attribute_definitions',
    'Attribute definitions',
    'center',
    1,
    '{}'::jsonb
WHERE EXISTS (
    SELECT 1 FROM workspaces WHERE id = '018f2000-0000-7000-8000-000000000002'::uuid
)
AND NOT EXISTS (
    SELECT 1 FROM views
    WHERE workspace_id = '018f2000-0000-7000-8000-000000000002'::uuid
      AND type_key = 'attribute_definitions'
);

-- Place the view beside Attributes when that tab is in the layout.
-- Otherwise append it to the first tab strip so the Data model menu shows it.
CREATE OR REPLACE FUNCTION braian_layout_add_view(node jsonb, view_id text, anchor text)
RETURNS jsonb
LANGUAGE plpgsql
AS $$
DECLARE
    kind text;
    views jsonb;
    children jsonb;
    child jsonb;
    next jsonb;
    i int;
    n int;
    inserted boolean := false;
BEGIN
    IF node IS NULL OR jsonb_typeof(node) <> 'object' THEN
        RETURN node;
    END IF;
    IF position(view_id IN node::text) > 0 THEN
        RETURN node;
    END IF;
    kind := COALESCE(node->>'type', '');
    IF kind = 'tabs' THEN
        views := COALESCE(node->'views', '[]'::jsonb);
        IF anchor = '' OR views @> to_jsonb(anchor) THEN
            RETURN jsonb_set(node, '{views}', views || to_jsonb(view_id));
        END IF;
        RETURN node;
    END IF;
    IF jsonb_typeof(node->'children') = 'array' THEN
        children := '[]'::jsonb;
        n := jsonb_array_length(node->'children');
        FOR i IN 0 .. n - 1 LOOP
            child := node->'children'->i;
            IF NOT inserted THEN
                next := braian_layout_add_view(child, view_id, anchor);
                IF next IS DISTINCT FROM child THEN
                    inserted := true;
                    child := next;
                END IF;
            END IF;
            children := children || jsonb_build_array(child);
        END LOOP;
        IF inserted THEN
            RETURN jsonb_set(node, '{children}', children);
        END IF;
    END IF;
    RETURN node;
END;
$$;

UPDATE workspaces
SET layout = COALESCE(
    NULLIF(
        braian_layout_add_view(
            layout,
            '018f2000-0000-7000-8000-000000000204',
            '018f2000-0000-7000-8000-000000000202'
        ),
        layout
    ),
    braian_layout_add_view(layout, '018f2000-0000-7000-8000-000000000204', '')
)
WHERE id = '018f2000-0000-7000-8000-000000000002'::uuid
  AND layout::text NOT LIKE '%018f2000-0000-7000-8000-000000000204%';

UPDATE workspace_states
SET document = jsonb_set(
    document,
    '{layout}',
    COALESCE(
        NULLIF(
            braian_layout_add_view(
                document->'layout',
                '018f2000-0000-7000-8000-000000000204',
                '018f2000-0000-7000-8000-000000000202'
            ),
            document->'layout'
        ),
        braian_layout_add_view(
            document->'layout',
            '018f2000-0000-7000-8000-000000000204',
            ''
        )
    )
)
WHERE workspace_id = '018f2000-0000-7000-8000-000000000002'::uuid
  AND document ? 'layout'
  AND document::text NOT LIKE '%018f2000-0000-7000-8000-000000000204%';

DROP FUNCTION braian_layout_add_view(jsonb, text, text);
