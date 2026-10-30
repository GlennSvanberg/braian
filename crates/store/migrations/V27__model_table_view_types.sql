-- Standard Data model tables (Object types, Link types, Value lists, Units).
-- The web template creates these view types; they need registry rows for FK.

INSERT INTO view_types (key, title, builtin) VALUES
  ('object_types', 'Object types', true),
  ('link_types', 'Link types', true),
  ('value_lists', 'Value lists', true),
  ('value_list_values', 'List values', true),
  ('units', 'Units', true)
ON CONFLICT (key) DO UPDATE SET title = EXCLUDED.title;
