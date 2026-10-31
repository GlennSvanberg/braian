-- The Review workspace lists every proposal in one table; the Proposal view works through one.
INSERT INTO view_types (key, title, builtin) VALUES
  ('proposal_list', 'Proposals', true)
ON CONFLICT (key) DO NOTHING;
