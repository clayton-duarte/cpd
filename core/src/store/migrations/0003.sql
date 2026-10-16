-- Migration 3 (contract §4.5, R34 A6 and B2): Plans across repos. The table
-- and trigger texts are §4.4's as written. schema.rs runs it with foreign keys
-- off and legacy_alter_table on (step 1), in two parts split at the line
-- `-- @fill-prs`: between them it generates the history triggers of `prs` and
-- `pr_needs` and fills `prs` with one PR per Plan (step 2); after the second
-- part it generates the history triggers of `plans`, `jobs` and
-- `context_records` (step 4). Frozen: the schema hash of version 3 covers this
-- text. Change the schema only with a new migration.

-- ---------- step 2: the PR tables ----------
CREATE TABLE prs (
  plan               TEXT NOT NULL REFERENCES plans(id),
  key                TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 24),
  repo               TEXT NOT NULL CHECK (length(repo) >= 1),
  base               TEXT NOT NULL CHECK (length(base) >= 1),
  status             TEXT NOT NULL CHECK (status IN ('waiting', 'dispatch', 'open', 'merged', 'cancelled')),
  lead_branch        TEXT NOT NULL CHECK (lead_branch IN ('cpd/' || plan || '/' || key, 'cpd/' || plan)),  -- the second form only for PRs made by migration 3
  lead_worktree      TEXT,
  pr_number          INTEGER,
  pr_url             TEXT,
  pr_opened_at       TEXT,
  pr_seen            TEXT NOT NULL DEFAULT '{"checks":{},"reviewIds":[],"commentIds":[]}' CHECK (json_valid(pr_seen)),
  pr_polled_at       TEXT,
  pr_title           TEXT CHECK (pr_title IS NULL OR length(pr_title) >= 1),
  pr_review_decision TEXT CHECK (pr_review_decision IN ('none', 'approved', 'changes_requested')),
  pr_checks          TEXT CHECK (pr_checks IN ('none', 'passed', 'pending', 'failed')),
  all_merged_at      TEXT,
  start_failed_at    TEXT,
  created_txn        INTEGER NOT NULL REFERENCES txns(id),
  created_at         TEXT NOT NULL,
  started_at         TEXT,
  merged_at          TEXT,
  PRIMARY KEY (plan, key),
  UNIQUE (plan, lead_branch),
  CHECK ((status IN ('waiting', 'cancelled')) = (lead_worktree IS NULL)),
  CHECK ((status IN ('waiting', 'cancelled')) = (started_at IS NULL)),
  CHECK ((status IN ('open', 'merged')) = (pr_number IS NOT NULL)),
  CHECK ((pr_number IS NULL) = (pr_url IS NULL) AND (pr_number IS NULL) = (pr_opened_at IS NULL)),
  CHECK ((pr_title IS NULL) = (pr_review_decision IS NULL) AND (pr_title IS NULL) = (pr_checks IS NULL)),
  CHECK ((status = 'merged') = (merged_at IS NOT NULL)),
  CHECK (start_failed_at IS NULL OR status = 'waiting')
) STRICT;

CREATE TABLE pr_needs (
  plan  TEXT NOT NULL,
  pr    TEXT NOT NULL,
  needs TEXT NOT NULL,
  PRIMARY KEY (plan, pr, needs),
  FOREIGN KEY (plan, pr)    REFERENCES prs(plan, key),
  FOREIGN KEY (plan, needs) REFERENCES prs(plan, key),
  CHECK (pr <> needs)
) STRICT;

-- @fill-prs

-- ---------- step 3: rebuild plans, jobs, events and context_records ----------
CREATE TABLE plans_new (
  id            TEXT PRIMARY KEY CHECK (length(id) BETWEEN 2 AND 24),
  context       TEXT NOT NULL REFERENCES contexts(id),
  title         TEXT NOT NULL CHECK (length(title) >= 1),
  jira          TEXT,
  phase         TEXT NOT NULL CHECK (phase IN ('grounding', 'plan', 'dispatch', 'pr', 'done')),
  lead_session  TEXT NOT NULL REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
  created_at    TEXT NOT NULL,
  locked_at     TEXT,
  done_at       TEXT,
  CHECK ((phase IN ('grounding', 'plan')) = (locked_at IS NULL)),
  CHECK ((phase = 'done') = (done_at IS NOT NULL))
) STRICT;
INSERT INTO plans_new (id, context, title, jira, phase, lead_session, created_at, locked_at, done_at)
  SELECT id, context, title, jira, phase, lead_session, created_at, locked_at, done_at FROM plans;
DROP TABLE plans;
ALTER TABLE plans_new RENAME TO plans;

CREATE TABLE jobs_new (
  seq                 INTEGER PRIMARY KEY AUTOINCREMENT,
  plan                TEXT NOT NULL REFERENCES plans(id),
  pr                  TEXT NOT NULL,          -- R34: the Job's PR (A7)
  review              INTEGER NOT NULL DEFAULT 0 CHECK (review IN (0, 1)),  -- B2: a review Job (added by migration 3)
  key                 TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 32),
  title               TEXT NOT NULL CHECK (length(title) >= 1),
  tier                TEXT NOT NULL CHECK (length(tier) = 2 AND substr(tier, 1, 1) IN ('O', 'S', 'H') AND substr(tier, 2, 1) BETWEEN '0' AND '5'),
  goal                TEXT NOT NULL CHECK (length(goal) >= 1),
  context             TEXT NOT NULL,
  touches             TEXT NOT NULL CHECK (json_valid(touches) AND json_type(touches) = 'array' AND json_array_length(touches) >= 1),
  acceptance          TEXT NOT NULL CHECK (json_valid(acceptance) AND json_type(acceptance) = 'array' AND json_array_length(acceptance) >= 1),
  out_of_scope        TEXT NOT NULL CHECK (json_valid(out_of_scope) AND json_type(out_of_scope) = 'array'),
  status              TEXT NOT NULL CHECK (status IN ('draft', 'queued', 'running', 'merged', 'failed', 'cancelled')),
  appended            INTEGER NOT NULL CHECK (appended IN (0, 1)),
  fingerprint         TEXT CHECK (fingerprint IS NULL OR length(fingerprint) = 64),
  created_txn         INTEGER NOT NULL REFERENCES txns(id),
  retry_tier          TEXT,
  cancel_reason       TEXT,
  cancel_requested_at TEXT,
  failed_cause        TEXT,
  merged_sha          TEXT,
  created_at          TEXT NOT NULL,
  UNIQUE (plan, key),
  FOREIGN KEY (plan, pr) REFERENCES prs(plan, key),
  CHECK (status <> 'draft' OR fingerprint IS NULL),
  CHECK (status IN ('draft', 'cancelled') OR fingerprint IS NOT NULL),
  CHECK ((status = 'merged') = (merged_sha IS NOT NULL)),
  CHECK ((status = 'failed') = (failed_cause IS NOT NULL)),
  CHECK ((cancel_reason IS NULL) = (cancel_requested_at IS NULL)),
  CHECK (status <> 'cancelled' OR cancel_reason IS NOT NULL)
) STRICT;
-- Every Job gets the key of its Plan's one PR (step 2), and is no review Job.
INSERT INTO jobs_new (seq, plan, pr, review, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                      status, appended, fingerprint, created_txn, retry_tier, cancel_reason, cancel_requested_at,
                      failed_cause, merged_sha, created_at)
  SELECT seq, plan, (SELECT prs.key FROM prs WHERE prs.plan = jobs.plan), 0, key, title, tier, goal, context,
         touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, retry_tier, cancel_reason,
         cancel_requested_at, failed_cause, merged_sha, created_at
  FROM jobs;
DROP TABLE jobs;
ALTER TABLE jobs_new RENAME TO jobs;

CREATE TABLE events_new (
  id      INTEGER PRIMARY KEY AUTOINCREMENT,
  txn     INTEGER NOT NULL REFERENCES txns(id),
  at      TEXT    NOT NULL,
  kind    TEXT    NOT NULL CHECK (kind IN (
            'context-created', 'context-record', 'context-grounded',
            'plan-created', 'plan-phase', 'plan-review', 'plan-done',
            'job-added', 'job-edited', 'job-merged', 'job-failed', 'job-retried',
            'job-cancel-requested', 'job-cancelled',
            'attempt-started', 'attempt-status', 'attempt-retry', 'attempt-nudge',
            'all-merged', 'pr-feedback',
            'pr-added', 'pr-edited', 'pr-cancelled', 'pr-started', 'pr-opened', 'pr-merged',
            'escalation', 'escalation-answered', 'escalation-expired',
            'session-registered', 'session-state', 'session-moved', 'session-renamed',
            'watch-started', 'watch-stopped', 'error')),
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
  SELECT id, txn, at, kind, context, plan, job, attempt, session, text, data FROM events;
DROP TABLE events;
ALTER TABLE events_new RENAME TO events;

CREATE TABLE context_records_new (               -- index of the git store (§4.6); rebuildable
  seq          INTEGER PRIMARY KEY AUTOINCREMENT,   -- local index position
  context      TEXT NOT NULL REFERENCES contexts(id),
  id           TEXT NOT NULL,
  kind         TEXT NOT NULL CHECK (kind IN ('decision', 'note', 'question', 'answer', 'plan')),
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
  CHECK ((plan_id IS NOT NULL) = (kind = 'plan')),
  CHECK ((plan_id IS NULL) = (plan_machine IS NULL))
) STRICT;
INSERT INTO context_records_new (seq, context, id, kind, at, machine, by_session, by_plan, text, supersedes,
                                 question, plan_id, plan_machine, plan_jira)
  SELECT seq, context, id, kind, at, machine, by_session, by_plan, text, supersedes,
         question, plan_id, plan_machine, plan_jira
  FROM context_records;
DROP TABLE context_records;
ALTER TABLE context_records_new RENAME TO context_records;

-- ---------- step 4: indexes and fixed triggers of the rebuilt and new tables ----------
CREATE INDEX events_plan ON events(plan, id);
CREATE INDEX events_context ON events(context, id);
CREATE INDEX events_session ON events(session, id);

CREATE TRIGGER pr_needs_no_delete BEFORE DELETE ON pr_needs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
CREATE TRIGGER pr_needs_hist_del AFTER DELETE ON pr_needs BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'pr_needs',
    json_array(OLD.plan, OLD.pr, OLD.needs), 'delete',
    json_object('plan', OLD.plan, 'pr', OLD.pr, 'needs', OLD.needs), NULL);
END;

CREATE TRIGGER jobs_locked BEFORE UPDATE OF title, tier, goal, context, touches, acceptance, out_of_scope, plan, key, pr, review ON jobs
WHEN OLD.status <> 'draft'
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;

CREATE TRIGGER prs_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON prs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;

CREATE TRIGGER pr_needs_locked BEFORE INSERT ON pr_needs
WHEN (SELECT phase FROM plans WHERE id = NEW.plan) NOT IN ('grounding', 'plan')
 AND (SELECT created_txn FROM prs WHERE plan = NEW.plan AND key = NEW.pr) <> cpd_txn()
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;

CREATE TRIGGER dependencies_same_pr BEFORE INSERT ON dependencies
WHEN (SELECT pr FROM jobs WHERE plan = NEW.plan AND key = NEW.job)
  IS NOT (SELECT pr FROM jobs WHERE plan = NEW.plan AND key = NEW.needs)
BEGIN SELECT RAISE(ABORT, 'needs_other_pr'); END;

CREATE TRIGGER jobs_pr_dependencies BEFORE UPDATE OF pr ON jobs
WHEN OLD.pr <> NEW.pr AND EXISTS (SELECT 1 FROM dependencies WHERE plan = NEW.plan AND (job = NEW.key OR needs = NEW.key))
BEGIN SELECT RAISE(ABORT, 'needs_other_pr'); END;

CREATE TRIGGER plans_phase BEFORE UPDATE OF phase ON plans
WHEN OLD.phase <> NEW.phase AND (OLD.phase || '>' || NEW.phase) NOT IN
  ('grounding>plan', 'plan>dispatch', 'dispatch>pr', 'pr>done')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER prs_status BEFORE UPDATE OF status ON prs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('waiting>dispatch', 'dispatch>open', 'open>merged', 'waiting>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER jobs_status BEFORE UPDATE OF status ON jobs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('draft>queued', 'queued>running', 'running>merged',
   'running>queued', 'running>failed', 'failed>queued',
   'draft>cancelled', 'queued>cancelled', 'failed>cancelled', 'running>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER context_records_immutable BEFORE UPDATE ON context_records
BEGIN SELECT RAISE(ABORT, 'append_only: context_records'); END;

-- After the copy above: the copied Events carry their old txn.
CREATE TRIGGER events_own BEFORE INSERT ON events WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'events.txn must be cpd_txn()'); END;
CREATE TRIGGER events_no_update  BEFORE UPDATE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
CREATE TRIGGER events_no_delete  BEFORE DELETE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
