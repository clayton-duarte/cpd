-- cpd-migration: additive
-- The Event kind `attempt-aborted` (contract §5.2, hs-a8): `attempt abort` (commands.md §6.5) appends exactly one
-- when the Lead ends a running Attempt `failed` / `aborted`.
INSERT INTO event_kinds (name) VALUES ('attempt-aborted');
