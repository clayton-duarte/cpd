-- cpd-migration: additive
-- The Event kind `session-model-changed` (contract §5.2, §4.5; the tier-preset switch, commands.md §14.6): `cpd
-- preset apply` rewrote the machine host `[model]`, and the adapter of a changed live Session switches pi's model
-- at its next idle.
INSERT INTO event_kinds (name) VALUES ('session-model-changed');
