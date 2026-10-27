-- cpd-migration: additive
-- A failed model switch (commands.md §10.16, §10.18a; Plan retier-truth, pro-d86): the JSON object
-- {"tier", "code", "message", "at"} the pi adapter records with `session switch-failed` when `setModel` fails, NULL when
-- no switch failed. `tier` stays the target. Existing rows get NULL; none is rewritten. The reconcile (K4) regenerates
-- `sessions`' history triggers from the new column.
ALTER TABLE sessions ADD COLUMN switch_failed TEXT CHECK (switch_failed IS NULL OR json_valid(switch_failed));
