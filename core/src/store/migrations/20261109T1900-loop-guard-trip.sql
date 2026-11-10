-- cpd-migration: additive
-- The Event kind `loop-guard-trip` (contract §5.2, §6.16 "The trip's cause and its report"): a loop-guard breaker
-- tripped, with its kind and count. A kind exists when its `event_kinds` row does (§4.4 K1): one additive statement.
INSERT INTO event_kinds (name) VALUES ('loop-guard-trip');
