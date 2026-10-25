-- cpd-migration: additive
-- Incident mode (hs-d634; contract §2.2 "Incident", commands.md §25): one row while incident mode is on.
-- The history triggers are generated for `incident` (no_delete 0: `incident end` deletes the row).
CREATE TABLE incident (
  id          INTEGER PRIMARY KEY CHECK (id = 1),   -- enforce one active row
  started_at  TEXT NOT NULL,
  started_by  TEXT NOT NULL,                         -- the Manager's Session id
  reason      TEXT NOT NULL CHECK (length(reason) >= 1),
  started_txn INTEGER NOT NULL REFERENCES txns(id)
) STRICT;
INSERT INTO history_tables (name, no_delete) VALUES ('incident', 0);
INSERT INTO event_kinds (name) VALUES ('incident-started'), ('incident-ended');
