-- cpd-migration: additive
-- The Event kind `incident-plans` (contract §5.2, hs-d664): a `cpd incident start --plan` on a live
-- incident appended it when the Plan set or `until_pr` changed.
INSERT INTO event_kinds (name) VALUES ('incident-plans');
