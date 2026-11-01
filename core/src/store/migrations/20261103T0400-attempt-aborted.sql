-- cpd-migration: breaking after 91
-- `aborted` joins the `attempts.cause` and `attempts.stop_request` CHECKs (contract §3.4 "attempt abort",
-- commands.md §6.5, hs-a8): `attempt abort` ends a running Attempt `failed` / `aborted` and, when a merge step
-- is running, sets `stop_request = 'aborted'`. `ALTER TABLE` cannot change a CHECK, so `attempts` is rebuilt as
-- `lead-branch-name` rebuilt it (the runner applies a migration with foreign keys off and `legacy_alter_table`
-- on): the same definition with `aborted` added to both CHECKs, a copy of every row and column (every id kept),
-- `DROP`, `RENAME`, indexes and the two triggers the rebuild dropped. The copy writes no `history` row.
--
-- Breaking (A1): a rebuild, and an old binary would refuse, at its CHECK, an Attempt that ended `aborted`.

CREATE TABLE attempts_new (
  plan             TEXT NOT NULL,
  id               TEXT NOT NULL,
  job              TEXT NOT NULL,
  number           INTEGER NOT NULL CHECK (number >= 1),
  tier             TEXT NOT NULL CHECK (length(tier) = 2),
  origin           TEXT NOT NULL CHECK (origin IN ('ladder', 'retry')),
  status           TEXT NOT NULL CHECK (status IN ('starting', 'running', 'merging', 'ended')),
  outcome          TEXT CHECK (outcome IN ('merged', 'failed', 'cancelled')),
  cause            TEXT CHECK (cause IN ('merged', 'cancel_requested', 'setup_failed', 'timed_out',
                                         'crashed', 'no_submit', 'merge_conflict', 'aborted',
                                         'cas_lost', 'dirty_target', 'tampered',
                                         'gate_failed', 'gate_error', 'provider_error')),
  branch           TEXT NOT NULL CHECK (branch = 'cpd/' || plan || '--' || id
                                        OR substr(branch, 1, length(plan || '/')) = plan || '/'
                                        OR branch = 'cpd/' || plan
                                        OR substr(branch, 1, length('cpd/' || plan || '/')) = 'cpd/' || plan || '/'),
  worktree         TEXT NOT NULL,
  base_sha         TEXT,
  driver_pid       INTEGER,
  worker_session   TEXT REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
  spec_path        TEXT,
  submitted_sha    TEXT,
  submitted_at     TEXT,
  check_tail       TEXT,
  merged_sha       TEXT,
  nudged           INTEGER NOT NULL DEFAULT 0 CHECK (nudged IN (0, 1)),
  stop_request     TEXT CHECK (stop_request IN ('timed_out', 'cancelled', 'aborted')),
  metrics          TEXT CHECK (metrics IS NULL OR json_valid(metrics)),
  started_at       TEXT NOT NULL,
  ended_at         TEXT,
  runner           TEXT NOT NULL DEFAULT 'pi' CHECK (runner IN ('pi', 'durable')),
  worker_pid       INTEGER,
  gate_hook        TEXT CHECK (gate_hook IN ('pre-commit', 'pre-merge')),
  gate_command     TEXT,
  gate_exit        INTEGER,
  actual_files     TEXT CHECK (actual_files IS NULL OR (json_valid(actual_files) AND json_type(actual_files) = 'array')),
  PRIMARY KEY (plan, id),
  UNIQUE (plan, job, number),
  FOREIGN KEY (plan, job) REFERENCES jobs(plan, key),
  CHECK (id = job || '-a' || number),
  CHECK ((status = 'ended') = (outcome IS NOT NULL)),
  CHECK ((outcome IS NULL) = (cause IS NULL)),
  CHECK ((status = 'ended') = (ended_at IS NOT NULL)),
  CHECK ((outcome = 'merged') = (cause = 'merged')),
  CHECK ((outcome = 'cancelled') = (cause = 'cancel_requested')),
  CHECK ((outcome = 'merged') = (merged_sha IS NOT NULL)),
  CHECK ((submitted_sha IS NULL) = (submitted_at IS NULL)),
  CHECK (status <> 'merging' OR submitted_sha IS NOT NULL)
) STRICT;
INSERT INTO attempts_new (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha,
                          driver_pid, worker_session, spec_path, submitted_sha, submitted_at, check_tail, merged_sha,
                          nudged, stop_request, metrics, started_at, ended_at, runner, worker_pid, gate_hook,
                          gate_command, gate_exit, actual_files)
  SELECT plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha,
         driver_pid, worker_session, spec_path, submitted_sha, submitted_at, check_tail, merged_sha,
         nudged, stop_request, metrics, started_at, ended_at, runner, worker_pid, gate_hook,
         gate_command, gate_exit, actual_files
  FROM attempts;
DROP TABLE attempts;
ALTER TABLE attempts_new RENAME TO attempts;
CREATE UNIQUE INDEX attempts_one_active ON attempts(plan, job) WHERE status <> 'ended';
CREATE INDEX attempts_active ON attempts(status) WHERE status <> 'ended';
CREATE TRIGGER attempts_status BEFORE UPDATE OF status ON attempts
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('starting>running', 'starting>ended', 'running>merging', 'running>ended', 'merging>ended')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
CREATE TRIGGER attempts_ended BEFORE UPDATE ON attempts
WHEN OLD.status = 'ended'
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
