-- Display names for the demo assortment. The wire key stays `sortiment`
-- so placements such as `sortiment:talt` keep working.

UPDATE definition_revisions AS rev
SET definitions = jsonb_set(
    rev.definitions,
    '{structures}',
    COALESCE((
        SELECT jsonb_agg(patched.structure ORDER BY patched.ordinality)
        FROM (
            SELECT
                element.ordinality,
                CASE
                    WHEN element.structure->>'key' <> 'sortiment' THEN element.structure
                    ELSE jsonb_set(
                        jsonb_set(
                            element.structure,
                            '{labels}',
                            '{"de":"eCom","en":"eCom","sv":"eCom"}'::jsonb,
                            true
                        ),
                        '{groups}',
                        COALESCE((
                            SELECT jsonb_agg(
                                CASE group_row.item->>'key'
                                    WHEN 'friluft' THEN jsonb_set(group_row.item, '{labels}', '{"de":"Outdoor","en":"Outdoor","sv":"Friluft"}'::jsonb, true)
                                    WHEN 'talt' THEN jsonb_set(group_row.item, '{labels}', '{"de":"Zelte","en":"Tents","sv":"Tält"}'::jsonb, true)
                                    WHEN 'sovsack' THEN jsonb_set(group_row.item, '{labels}', '{"de":"Schlafsäcke","en":"Sleeping bags","sv":"Sovsäck"}'::jsonb, true)
                                    WHEN 'bygg' THEN jsonb_set(group_row.item, '{labels}', '{"de":"Bau","en":"Building","sv":"Bygg"}'::jsonb, true)
                                    WHEN 'virke' THEN jsonb_set(group_row.item, '{labels}', '{"de":"Holz","en":"Timber","sv":"Virke"}'::jsonb, true)
                                    ELSE group_row.item
                                END
                                ORDER BY group_row.ordinality
                            )
                            FROM jsonb_array_elements(element.structure->'groups') WITH ORDINALITY AS group_row(item, ordinality)
                        ), '[]'::jsonb),
                        true
                    )
                END AS structure
            FROM jsonb_array_elements(rev.definitions->'structures') WITH ORDINALITY AS element(structure, ordinality)
        ) AS patched
    ), '[]'::jsonb)
)
WHERE jsonb_typeof(rev.definitions->'structures') = 'array';
