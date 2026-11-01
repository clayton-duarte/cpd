-- cpd-migration: additive
-- The Event kind `attempt-ended` (contract §5.2, hs-d798): one Event per end of a `pi` or `durable`
-- Attempt, appended in the same transaction that writes `attempts.metrics`, with the Attempt's
-- metrics blob as its `data`.
INSERT INTO event_kinds (name) VALUES ('attempt-ended');
