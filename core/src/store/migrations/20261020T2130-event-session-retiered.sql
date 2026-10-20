-- cpd-migration: additive
-- The Event kind `session-retiered` (contract §5.2, §4.5 `event-session-retiered`; hs-d506): `session retier` moved a
-- Lead's or a Manager's Tier, and the adapter of that Session switches pi's model at its next idle.
INSERT INTO event_kinds (name) VALUES ('session-retiered');
