-- cpd-migration: breaking after 27
-- A note may supersede a note, and a decision a decision or a note (contract §2.2 "Context record",
-- §4.6 "A write"; recency wins). The CHECK `supersedes IS NULL OR kind = 'decision'` becomes
-- `supersedes IS NULL OR kind IN ('decision', 'note')`; the pairing of kinds (a note supersedes only
-- a note, a decision a decision or a note) is checked by the binary at the write.
--
-- `ALTER TABLE` cannot change a CHECK, so `context_records` is rebuilt as `kind-tables` rebuilt it:
-- new table with the §4.4 definition, copy of every row and column, `DROP`, `RENAME`, the immutability
-- trigger. The history triggers of `context_records` are derived (§4.5 K4): the runner's reconcile
-- creates them again after the last migration. The copy writes no `history` row.
--
-- Breaking (A1): a rebuild; and an older binary that meets a note with `supersedes` would refuse to
-- parse the record it reads.

CREATE TABLE context_records_new (
  seq          INTEGER PRIMARY KEY AUTOINCREMENT,
  context      TEXT NOT NULL REFERENCES contexts(id),
  id           TEXT NOT NULL,
  kind         TEXT NOT NULL REFERENCES record_kinds(name),
  at           TEXT NOT NULL,
  machine      TEXT NOT NULL,
  by_session   TEXT,
  by_plan      TEXT,
  text         TEXT NOT NULL CHECK (length(text) >= 1),
  supersedes   TEXT,
  question     TEXT,
  plan_id      TEXT,
  plan_machine TEXT,
  plan_jira    TEXT,
  UNIQUE (context, id),
  CHECK (supersedes IS NULL OR kind IN ('decision', 'note')),
  CHECK ((question IS NOT NULL) = (kind = 'answer')),
  CHECK ((plan_id IS NOT NULL) = (kind IN ('plan', 'brief'))),
  CHECK ((plan_id IS NULL) = (plan_machine IS NULL))
) STRICT;
INSERT INTO context_records_new (seq, context, id, kind, at, machine, by_session, by_plan, text, supersedes,
                                 question, plan_id, plan_machine, plan_jira)
  SELECT seq, context, id, kind, at, machine, by_session, by_plan, text, supersedes,
         question, plan_id, plan_machine, plan_jira
  FROM context_records ORDER BY seq;
INSERT INTO sqlite_sequence (name, seq)
  SELECT 'context_records_new', seq FROM sqlite_sequence
  WHERE name = 'context_records' AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'context_records_new');
UPDATE sqlite_sequence SET seq = max(seq, coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'context_records'), 0))
  WHERE name = 'context_records_new';
DROP TABLE context_records;
ALTER TABLE context_records_new RENAME TO context_records;
CREATE TRIGGER context_records_immutable BEFORE UPDATE ON context_records
BEGIN SELECT RAISE(ABORT, 'append_only: context_records'); END;
