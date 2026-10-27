-- cpd-migration: breaking after 70
-- The Lead Branch of a PR is `<plan>/<key>`, without the `cpd/` prefix (contract §2.1, §4.4, decision pro-d112,
-- assumptions pro-d113 A1-A3). `ALTER TABLE` cannot change a CHECK, so `prs` and `attempts` are rebuilt (the runner
-- applies a migration with foreign keys off and `legacy_alter_table` on):
--   * `prs.lead_branch` accepts `<plan>/<key>` (a new PR) besides the two legacy forms `cpd/<plan>/<key>` and
--     `cpd/<plan>`, which stay valid and are never renamed (A1);
--   * `attempts.branch` accepts `cpd/<plan>--<id>` (a parallel Attempt) or a Lead Branch of the Plan in any of
--     the three forms (a linear Attempt's branch is its PR's `lead_branch`);
--   * `prs_locked` loses the exception that let `pr add` move a merged PR's `cpd/<plan>` to `cpd/<plan>/<key>`
--     (A3: that move is deleted).
-- Every row and column is copied, no row is rewritten and no `history` row is written; the history triggers of
-- `prs` are the reconcile's.
--
-- Breaking: a rebuild, and an old binary would refuse, at its CHECK, a PR or an Attempt on a `<plan>/<key>` branch.

CREATE TABLE prs_new (
  plan               TEXT NOT NULL REFERENCES plans(id),
  key                TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 24),
  repo               TEXT NOT NULL CHECK (length(repo) >= 1),
  base               TEXT NOT NULL CHECK (length(base) >= 1),
  status             TEXT NOT NULL CHECK (status IN ('waiting', 'dispatch', 'open', 'merged', 'cancelled')),
  lead_branch        TEXT NOT NULL CHECK (lead_branch IN (plan || '/' || key, 'cpd/' || plan || '/' || key, 'cpd/' || plan)),
  lead_worktree      TEXT,
  pr_number          INTEGER,
  pr_url             TEXT,
  pr_opened_at       TEXT,
  pull_request       INTEGER REFERENCES pull_requests(id),
  all_merged_at      TEXT,
  start_failed_at    TEXT,
  created_txn        INTEGER NOT NULL REFERENCES txns(id),
  created_at         TEXT NOT NULL,
  started_at         TEXT,
  merged_at          TEXT,
  name               TEXT CHECK (name IS NULL OR length(name) BETWEEN 1 AND 100),
  title              TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 72),
  shape              TEXT NOT NULL DEFAULT 'free' CHECK (shape IN ('tdd', 'docs', 'config', 'free')),
  head_ref           TEXT CHECK (head_ref IS NULL OR length(head_ref) BETWEEN 1 AND 255),
  description        TEXT CHECK (description IS NULL OR length(description) BETWEEN 1 AND 65536),
  PRIMARY KEY (plan, key),
  UNIQUE (plan, lead_branch),
  CHECK ((status IN ('waiting', 'cancelled')) = (lead_worktree IS NULL)),
  CHECK ((status IN ('waiting', 'cancelled')) = (started_at IS NULL)),
  CHECK ((status IN ('open', 'merged')) = (pr_number IS NOT NULL)),
  CHECK ((pr_number IS NULL) = (pr_url IS NULL) AND (pr_number IS NULL) = (pr_opened_at IS NULL)),
  CHECK ((status IN ('open', 'merged')) = (pull_request IS NOT NULL)),
  CHECK ((status = 'merged') = (merged_at IS NOT NULL)),
  CHECK (start_failed_at IS NULL OR status = 'waiting')
) STRICT;
INSERT INTO prs_new (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                     pull_request, all_merged_at, start_failed_at, created_txn, created_at, started_at, merged_at,
                     name, title, shape, head_ref, description)
  SELECT plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
         pull_request, all_merged_at, start_failed_at, created_txn, created_at, started_at, merged_at,
         name, title, shape, head_ref, description
  FROM prs;
DROP TABLE prs;
ALTER TABLE prs_new RENAME TO prs;
CREATE UNIQUE INDEX prs_head_ref_live ON prs (repo, head_ref) WHERE head_ref IS NOT NULL AND status IN ('waiting', 'dispatch', 'open');

-- A PR's identity, repo and base are Locked once its Plan is locked; the follow of `base` alone is the one exception.
CREATE TRIGGER prs_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON prs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT (OLD.status IN ('waiting', 'dispatch', 'open')
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.lead_branch IS OLD.lead_branch
          AND NEW.base IS NOT OLD.base
          AND (OLD.status = 'open'
               OR EXISTS (SELECT 1 FROM prs q WHERE q.repo = OLD.repo AND q.lead_branch = OLD.base)))
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
CREATE TRIGGER prs_status BEFORE UPDATE OF status ON prs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('waiting>dispatch', 'dispatch>open', 'open>merged', 'waiting>cancelled', 'dispatch>cancelled', 'open>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
CREATE TRIGGER prs_shape_fixed BEFORE UPDATE OF shape ON prs
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
CREATE TRIGGER prs_head_ref_fixed BEFORE UPDATE OF head_ref ON prs
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;

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
                                         'crashed', 'no_submit', 'merge_conflict',
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
  stop_request     TEXT CHECK (stop_request IN ('timed_out', 'cancelled')),
  metrics          TEXT CHECK (metrics IS NULL OR json_valid(metrics)),
  started_at       TEXT NOT NULL,
  ended_at         TEXT,
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
                          nudged, stop_request, metrics, started_at, ended_at)
  SELECT plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha,
         driver_pid, worker_session, spec_path, submitted_sha, submitted_at, check_tail, merged_sha,
         nudged, stop_request, metrics, started_at, ended_at
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
