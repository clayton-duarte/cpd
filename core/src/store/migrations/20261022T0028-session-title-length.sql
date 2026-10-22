-- cpd-migration: breaking after 38
-- The dynamic title's length (contract §2.2 Session `title`, §4.4, §4.5; hs-d543): a title is at most the chat slot's
-- width less 8 characters, up to a Title's maximum of 200, so `sessions.title`'s check goes from 1–60 to 1–200.
--
-- `ALTER TABLE` cannot change a CHECK, so `sessions` is rebuilt as `waiting-state` rebuilt it (the runner applies a
-- migration with foreign keys off and `legacy_alter_table` on): new table with the §4.4 definition, copy of every row
-- and column (every title kept), `DROP`, `RENAME`, indexes. The history triggers of `sessions` are derived (§4.5 K4):
-- the runner's reconcile creates them again after the last migration. The copy writes no `history` row.
--
-- Breaking (A1): a rebuild; and an older binary would refuse, at its CHECK, a title of more than 60 characters.

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
  state       TEXT NOT NULL CHECK (state IN ('working', 'waiting', 'blocked', 'idle', 'stopped')),
  state_label TEXT,
  state_at    TEXT NOT NULL,
  created_at  TEXT NOT NULL,
  archived_at TEXT,
  title       TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 200),
  titled_at   TEXT,
  asleep_at   TEXT,
  FOREIGN KEY (plan, attempt) REFERENCES attempts(plan, id) DEFERRABLE INITIALLY DEFERRED,
  CHECK ((role = 'worker') = (attempt IS NOT NULL)),
  CHECK (role = 'manager' OR role = 'lead' OR plan IS NOT NULL),
  CHECK (role <> 'manager' OR plan IS NULL),
  CHECK ((state IN ('waiting', 'blocked')) = (state_label IS NOT NULL))
) STRICT;
INSERT INTO sessions_new (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                          state, state_label, state_at, created_at, archived_at, title, titled_at, asleep_at)
  SELECT id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
         state, state_label, state_at, created_at, archived_at, title, titled_at, asleep_at
  FROM sessions;
DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;
CREATE INDEX sessions_plan ON sessions(plan);
CREATE UNIQUE INDEX sessions_one_manager ON sessions(context) WHERE role = 'manager' AND archived_at IS NULL;
CREATE INDEX sessions_live ON sessions(id) WHERE state <> 'stopped' OR tmux_pane IS NOT NULL;
