-- Migration 10 (contract §4.5, Plan manager B3, hs-d92, hs-d129, hs-d133, B3-A3):
-- the Manager role and the `brief` record; the grounded Lead is retired.
-- `ALTER TABLE` cannot change a CHECK or drop a column a CHECK names, so
-- `sessions`, `context_records` and `contexts` are rebuilt as migration 4 rebuilt
-- `plans`; the caller runs this inside the migrating transaction with foreign
-- keys off and `legacy_alter_table` on, generates the history triggers of the
-- three tables afterwards and archives the grounded Leads (step 4).

-- ---------- step 1: sessions ----------
CREATE TABLE sessions_new (
  id          TEXT PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 128),
  role        TEXT NOT NULL CHECK (role IN ('manager', 'lead', 'worker')),
  context     TEXT NOT NULL REFERENCES contexts(id),
  plan        TEXT REFERENCES plans(id) DEFERRABLE INITIALLY DEFERRED,
  attempt     TEXT,
  name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
  file        TEXT,
  forked_from TEXT REFERENCES sessions(id),
  cwd         TEXT NOT NULL,
  tier        TEXT NOT NULL CHECK (length(tier) = 2),
  tmux_pane   TEXT CHECK (tmux_pane IS NULL OR tmux_pane GLOB '%[0-9]*'),
  state       TEXT NOT NULL CHECK (state IN ('working', 'blocked', 'idle', 'stopped')),
  state_label TEXT,
  state_at    TEXT NOT NULL,
  created_at  TEXT NOT NULL,
  archived_at TEXT,
  FOREIGN KEY (plan, attempt) REFERENCES attempts(plan, id) DEFERRABLE INITIALLY DEFERRED,
  CHECK ((role = 'worker') = (attempt IS NOT NULL)),
  CHECK (role = 'manager' OR role = 'lead' OR plan IS NOT NULL),
  CHECK (role <> 'manager' OR plan IS NULL),
  CHECK ((state = 'blocked') = (state_label IS NOT NULL))
) STRICT;
INSERT INTO sessions_new (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                          state, state_label, state_at, created_at, archived_at)
  SELECT id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
         state, state_label, state_at, created_at, archived_at
  FROM sessions;
DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;
CREATE INDEX sessions_plan ON sessions(plan);
CREATE UNIQUE INDEX sessions_one_manager ON sessions(context) WHERE role = 'manager' AND archived_at IS NULL;

-- ---------- step 2: context_records ----------
CREATE TABLE context_records_new (               -- the only store of Context records (§4.6)
  seq          INTEGER PRIMARY KEY AUTOINCREMENT,   -- position in this machine's database
  context      TEXT NOT NULL REFERENCES contexts(id),
  id           TEXT NOT NULL,
  kind         TEXT NOT NULL CHECK (kind IN ('decision', 'note', 'question', 'answer', 'plan', 'brief')),
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
-- Positions never go back: the new table's sequence is at least the old one's (as migration 3).
UPDATE sqlite_sequence SET seq = max(seq, coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'context_records'), 0))
  WHERE name = 'context_records_new';
DROP TABLE context_records;
ALTER TABLE context_records_new RENAME TO context_records;
CREATE TRIGGER context_records_immutable BEFORE UPDATE ON context_records
BEGIN SELECT RAISE(ABORT, 'append_only: context_records'); END;

-- ---------- step 3: contexts without lead_session and grounded_seq ----------
CREATE TABLE contexts_new (
  id            TEXT PRIMARY KEY CHECK (length(id) BETWEEN 2 AND 40),
  title         TEXT NOT NULL CHECK (length(title) >= 1),
  jira          TEXT,
  created_at    TEXT NOT NULL,
  created_on    TEXT NOT NULL,
  archived_at   TEXT                          -- see migration 0010 (§4.5) for the columns it dropped
) STRICT;
INSERT INTO contexts_new (id, title, jira, created_at, created_on, archived_at)
  SELECT id, title, jira, created_at, created_on, archived_at FROM contexts;
DROP TABLE contexts;
ALTER TABLE contexts_new RENAME TO contexts;
