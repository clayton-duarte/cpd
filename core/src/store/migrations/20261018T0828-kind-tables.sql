-- cpd-migration: breaking after 11
-- The lookup tables for the lists that migrations extend (contract §4.4, §4.5
-- K1–K4; hs-d178 D1): `event_kinds`, `record_kinds` and `history_tables`, filled
-- with today's values. `events` and `context_records` are rebuilt once more, the
-- last time either is rebuilt for a kind: `kind` is a foreign key instead of a
-- CHECK, so a later kind is one `INSERT INTO event_kinds`, in an additive
-- migration. No history trigger is created here: the runner's reconcile
-- generates them for every table of `history_tables`, the three lookup tables
-- included. schema.rs runs it with foreign keys off and legacy_alter_table on.

CREATE TABLE event_kinds    (name TEXT PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 40)) STRICT, WITHOUT ROWID;
CREATE TABLE record_kinds   (name TEXT PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 40)) STRICT, WITHOUT ROWID;
CREATE TABLE history_tables (name TEXT PRIMARY KEY, no_delete INTEGER NOT NULL CHECK (no_delete IN (0, 1))) STRICT, WITHOUT ROWID;

INSERT INTO event_kinds (name) VALUES
  ('context-created'), ('context-record'), ('context-grounded'),
  ('plan-created'), ('plan-phase'), ('plan-review'), ('plan-done'),
  ('job-added'), ('job-edited'), ('job-merged'), ('job-failed'), ('job-retried'),
  ('job-cancel-requested'), ('job-cancelled'),
  ('attempt-started'), ('attempt-status'), ('attempt-retry'), ('attempt-nudge'),
  ('all-merged'), ('pr-feedback'),
  ('pr-added'), ('pr-edited'), ('pr-cancelled'), ('pr-started'), ('pr-opened'), ('pr-merged'),
  ('escalation'), ('escalation-answered'), ('escalation-expired'),
  ('session-registered'), ('session-state'), ('session-moved'), ('session-renamed'),
  ('context-renamed'), ('plan-renamed'),
  ('context-archived'), ('plan-archived'), ('session-archived'),
  ('session-message'),
  ('watch-started'), ('watch-stopped'), ('error');
INSERT INTO record_kinds (name) VALUES ('decision'), ('note'), ('question'), ('answer'), ('plan'), ('brief');
-- The tables whose history triggers are generated; `no_delete` 0: no generated `_no_delete`
-- (`pull_requests`, `pr_needs` and `dependencies` write their own rule).
INSERT INTO history_tables (name, no_delete) VALUES
  ('contexts', 1), ('context_records', 1), ('plans', 1), ('pull_requests', 0), ('prs', 1), ('pr_needs', 0),
  ('plan_reviews', 1), ('jobs', 1), ('dependencies', 0), ('attempts', 1), ('escalations', 1),
  ('sessions', 1), ('tier_catalog', 1), ('event_kinds', 1), ('record_kinds', 1), ('history_tables', 1);

-- ---------- events ----------
CREATE TABLE events_new (
  id      INTEGER PRIMARY KEY AUTOINCREMENT,
  txn     INTEGER NOT NULL REFERENCES txns(id),
  at      TEXT    NOT NULL,
  kind    TEXT    NOT NULL REFERENCES event_kinds(name),   -- §4.5: a kind exists when its row does
  context TEXT    REFERENCES contexts(id),
  plan    TEXT    REFERENCES plans(id),
  job     TEXT,
  attempt TEXT,
  session TEXT    REFERENCES sessions(id),
  text    TEXT    NOT NULL,
  data    TEXT    NOT NULL CHECK (json_valid(data) AND json_type(data) = 'object'),
  CHECK (plan IS NULL OR context IS NOT NULL)
) STRICT;
INSERT INTO events_new (id, txn, at, kind, context, plan, job, attempt, session, text, data)
  SELECT id, txn, at, kind, context, plan, job, attempt, session, text, data FROM events ORDER BY id;
-- Ids never go back: the new table's sequence is at least the old one's (as 0003 and 0004).
INSERT INTO sqlite_sequence (name, seq)
  SELECT 'events_new', seq FROM sqlite_sequence
  WHERE name = 'events' AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'events_new');
UPDATE sqlite_sequence SET seq = max(seq, coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'events'), 0))
  WHERE name = 'events_new';
DROP TABLE events;
ALTER TABLE events_new RENAME TO events;
CREATE INDEX events_plan ON events(plan, id);
CREATE INDEX events_context ON events(context, id);
CREATE INDEX events_session ON events(session, id);
-- After the copy above: the copied Events carry their old txn.
CREATE TRIGGER events_own BEFORE INSERT ON events WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'events.txn must be cpd_txn()'); END;
CREATE TRIGGER events_no_update  BEFORE UPDATE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
CREATE TRIGGER events_no_delete  BEFORE DELETE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;

-- ---------- context_records ----------
CREATE TABLE context_records_new (               -- the only store of Context records (§4.6)
  seq          INTEGER PRIMARY KEY AUTOINCREMENT,   -- position in this machine's database
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
  plan_jira    TEXT,                          -- R34: no plan_repo (A12)
  UNIQUE (context, id),
  CHECK (supersedes IS NULL OR kind = 'decision'),
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
