-- cpd-migration: additive
-- The six Workflow Event kinds (contract §5.2, hs-d805; rename map R6/R7a): the object's Events are `workflow-added`,
-- `workflow-edited`, `workflow-described`, `workflow-cancelled`, `workflow-started` and `workflow-base-changed`. The
-- retired `pr-*` rows stay in `event_kinds` as history (past Events hold those kinds, §4.5 K1), so the new names are
-- added beside them and nothing is rewritten.
INSERT INTO event_kinds (name) VALUES
  ('workflow-added'), ('workflow-edited'), ('workflow-described'),
  ('workflow-cancelled'), ('workflow-started'), ('workflow-base-changed');
