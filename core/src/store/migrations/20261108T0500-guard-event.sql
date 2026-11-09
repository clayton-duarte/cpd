-- cpd-migration: additive
-- The Event kind `guard` (contract §5.2, Plan `agent-refusals`): one of a Session's own tool calls or
-- commands was refused, by an adapter guard or by a cpd command's own invariant. A kind exists when its
-- `event_kinds` row does (§4.4 K1): one additive migration of one statement.
INSERT INTO event_kinds (name) VALUES ('guard');
