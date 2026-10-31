-- The seeded Review workspace now has a Proposals list (left) and a Proposal view (center).
-- Seeding never overwrites an existing view, so rename the old one here.
UPDATE views SET title = 'Proposal', pane = 'center'
WHERE id = '018f2000-0000-7000-8000-000000000401' AND type_key = 'proposal' AND title = 'Proposals';
