-- English titles for the built-in view types. V3 stays as applied.
UPDATE view_types SET title = 'Structure' WHERE key = 'structure_tree';
UPDATE view_types SET title = 'Objects' WHERE key = 'object_table';
UPDATE view_types SET title = 'Detail' WHERE key = 'object_detail';
UPDATE view_types SET title = 'Graph' WHERE key = 'graph';
UPDATE view_types SET title = 'Data model' WHERE key = 'data_model';
UPDATE view_types SET title = 'History' WHERE key = 'history';
UPDATE view_types SET title = 'Batch log' WHERE key = 'batch_log';
UPDATE view_types SET title = 'Jobs' WHERE key = 'job_history';
UPDATE view_types SET title = 'Proposals' WHERE key = 'proposal';
UPDATE view_types SET title = 'Embedded view' WHERE key = 'embed';

-- Overview is gone. Views of that type go first because of the foreign key.
DELETE FROM views WHERE type_key = 'dashboard';
DELETE FROM view_types WHERE key = 'dashboard';
