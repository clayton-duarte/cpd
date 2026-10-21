-- cpd-migration: additive
-- Asleep Sessions (contract §2.2 Session `asleep_at`, §3.7 E13, §4.5 `session-asleep`; hs-d515, hs-d520): when engine
-- E13 put a finished Lead or Manager to sleep, null until then. Rewrites no row. The reconcile (K4) regenerates
-- `sessions`' history triggers from the new column.
ALTER TABLE sessions ADD COLUMN asleep_at TEXT;

-- The Event kind `session-asleep` {} (contract §5.2).
INSERT INTO event_kinds (name) VALUES ('session-asleep');
