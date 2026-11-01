-- cpd-migration: breaking after 86
-- The host crash-loop label (contract §6.19, P1.10; hs-d785): the engine stops a durable Session whose host
-- restarted 3 times in 10 minutes with `state = 'stopped'` and `state_label = 'host crash loop'`. The label check
-- of `sessions` must allow a label on a stopped Session for that one value. `ALTER TABLE` cannot change a CHECK,
-- so `sessions` is rebuilt as `session-title-length` rebuilt it (the runner applies a migration with foreign keys
-- off and `legacy_alter_table` on): new table with the §4.4 definition, copy of every row and column, `DROP`,
-- `RENAME`, the indexes and the `working_pr` triggers again. The history triggers of `sessions` are derived
-- (§4.5 K4): the runner's reconcile creates them again after the last migration. The copy writes no `history` row.
--
-- Breaking (A1): a rebuild; a binary without it refuses, at its old CHECK, the label of a stopped Session.

CREATE TABLE sessions_new (
  id            TEXT PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 128),
  role          TEXT NOT NULL CHECK (role IN ('manager', 'lead', 'worker')),
  context       TEXT NOT NULL REFERENCES contexts(id),
  plan          TEXT REFERENCES plans(id) DEFERRABLE INITIALLY DEFERRED,
  attempt       TEXT,
  name          TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
  file          TEXT,
  forked_from   TEXT REFERENCES sessions(id),
  cwd           TEXT NOT NULL,
  tier          TEXT NOT NULL CHECK (length(tier) = 2),
  tmux_pane     TEXT CHECK (tmux_pane IS NULL OR tmux_pane GLOB '%[0-9]*'),
  state         TEXT NOT NULL CHECK (state IN ('working', 'waiting', 'blocked', 'idle', 'stopped')),
  state_label   TEXT,
  state_at      TEXT NOT NULL,
  created_at    TEXT NOT NULL,
  archived_at   TEXT,
  title         TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 200),
  titled_at     TEXT,
  asleep_at     TEXT,
  by_user       INTEGER NOT NULL DEFAULT 0 CHECK (by_user IN (0, 1)),
  switch_failed TEXT CHECK (switch_failed IS NULL OR json_valid(switch_failed)),
  working_pr    TEXT,
  runner        TEXT NOT NULL DEFAULT 'pi' CHECK (runner IN ('pi', 'durable')),
  FOREIGN KEY (plan, attempt) REFERENCES attempts(plan, id) DEFERRABLE INITIALLY DEFERRED,
  CHECK ((role = 'worker') = (attempt IS NOT NULL)),
  CHECK (role = 'manager' OR role = 'lead' OR plan IS NOT NULL),
  CHECK (role <> 'manager' OR plan IS NULL),
  CHECK (state NOT IN ('waiting', 'blocked') OR state_label IS NOT NULL),
  CHECK (state_label IS NULL OR state IN ('waiting', 'blocked') OR (state = 'stopped' AND state_label = 'host crash loop'))
) STRICT;
INSERT INTO sessions_new (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                          state, state_label, state_at, created_at, archived_at, title, titled_at, asleep_at,
                          by_user, switch_failed, working_pr, runner)
  SELECT id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
         state, state_label, state_at, created_at, archived_at, title, titled_at, asleep_at,
         by_user, switch_failed, working_pr, runner
  FROM sessions;
DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;
CREATE INDEX sessions_plan ON sessions(plan);
CREATE UNIQUE INDEX sessions_one_manager ON sessions(context) WHERE role = 'manager' AND archived_at IS NULL;
CREATE INDEX sessions_live ON sessions(id) WHERE state <> 'stopped' OR tmux_pane IS NOT NULL;
CREATE TRIGGER sessions_working_pr_new BEFORE INSERT ON sessions
WHEN NEW.working_pr IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'working_pr_invalid');
END;
CREATE TRIGGER sessions_working_pr_valid BEFORE UPDATE OF working_pr ON sessions
WHEN NEW.working_pr IS NOT NULL
  AND NOT (NEW.role = 'lead' AND NEW.state = 'working' AND EXISTS (SELECT 1 FROM prs WHERE plan = NEW.plan AND key = NEW.working_pr))
BEGIN
  SELECT RAISE(ABORT, 'working_pr_invalid');
END;
CREATE TRIGGER sessions_working_pr_clear AFTER UPDATE OF state ON sessions
WHEN OLD.state = 'working' AND NEW.state <> 'working' AND NEW.working_pr IS NOT NULL
BEGIN
  UPDATE sessions SET working_pr = NULL WHERE id = NEW.id;
END;
