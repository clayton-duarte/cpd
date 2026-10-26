-- cpd-migration: additive
-- The Event kind `mcp-attached` (contract §5.2, §4.5 `event-mcp-attached`; pro-d8): `cpd mcp attach` named a server of
-- the machine policy for one Session, and the adapter of that Session registers it with pi.
INSERT INTO event_kinds (name) VALUES ('mcp-attached');
