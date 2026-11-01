-- cpd-migration: additive
-- The Event kind `job-blocked` (contract §5.2, hs-d798): one Event per waiting episode of a Ready
-- Job that E3 skips for want of a pool slot, with the number of Ready Jobs skipped that tick as
-- its `data`.
INSERT INTO event_kinds (name) VALUES ('job-blocked');
