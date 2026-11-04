-- cpd-migration: additive
-- Which Context records a Session read, by which channel (pro-d10 item 5): additive telemetry, deletable with its Context.
CREATE TABLE context_reads (
  context TEXT NOT NULL REFERENCES contexts(id),
  record  TEXT NOT NULL,
  session TEXT NOT NULL CHECK (length(session) >= 1),
  channel TEXT NOT NULL CHECK (channel IN ('pushed','search','show','shadow')),
  read_at TEXT NOT NULL,
  rank    INTEGER CHECK (rank IS NULL OR rank >= 1),
  score   REAL,
  used    INTEGER CHECK (used IN (0,1)),
  PRIMARY KEY (context, record, session, channel),
  FOREIGN KEY (context, record) REFERENCES context_records(context, id)
) STRICT;
CREATE INDEX context_reads_record ON context_reads(context, record, read_at);
INSERT INTO history_tables (name, no_delete) VALUES ('context_reads', 0);
