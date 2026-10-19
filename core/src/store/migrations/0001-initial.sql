-- Migration 1 (contract §4.4): the tables, indexes and fixed triggers, exactly
-- as written there. The history triggers (`<table>_hist_ins`, `_hist_upd`,
-- `_no_delete`) are generated from the tables' columns by `schema.rs`.
-- Frozen: the schema hash of version 1 covers this text. Change the schema
-- only with a new migration.

-- ---------- audit ----------
CREATE TABLE txns (
  id      INTEGER PRIMARY KEY,               -- explicit: max(id) + 1 under BEGIN IMMEDIATE
  at      TEXT    NOT NULL,
  command TEXT    NOT NULL,                  -- e.g. 'job add', 'watch', 'attempt run', 'migrate'
  pid     INTEGER NOT NULL,
  session TEXT,                              -- CPD_SESSION or PI_SESSION_ID of the caller, if any
  cwd     TEXT    NOT NULL
) STRICT;

CREATE TABLE history (
  id     INTEGER PRIMARY KEY AUTOINCREMENT,
  txn    INTEGER NOT NULL REFERENCES txns(id),
  tbl    TEXT    NOT NULL,
  pk     TEXT    NOT NULL CHECK (json_valid(pk) AND json_type(pk) = 'array'),
  op     TEXT    NOT NULL CHECK (op IN ('insert', 'update', 'delete')),
  before TEXT    CHECK (before IS NULL OR json_valid(before)),
  after  TEXT    CHECK (after  IS NULL OR json_valid(after)),
  CHECK ((op = 'insert') = (before IS NULL)),
  CHECK ((op = 'delete') = (after IS NULL))
) STRICT;
CREATE INDEX history_tbl_pk ON history(tbl, pk);
CREATE INDEX history_txn ON history(txn);

-- ---------- Contexts ----------
CREATE TABLE contexts (
  id            TEXT PRIMARY KEY CHECK (length(id) BETWEEN 2 AND 40),
  title         TEXT NOT NULL CHECK (length(title) >= 1),
  jira          TEXT,
  created_at    TEXT NOT NULL,
  created_on    TEXT NOT NULL,
  lead_session  TEXT REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
  grounded_seq  INTEGER,
  sync_failing  INTEGER NOT NULL DEFAULT 0 CHECK (sync_failing IN (0, 1)),
  CHECK ((lead_session IS NULL) = (grounded_seq IS NULL))
) STRICT;

CREATE TABLE context_records (               -- index of the git store (§4.6); rebuildable
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
  plan_repo    TEXT,
  plan_jira    TEXT,
  UNIQUE (context, id),
  CHECK (supersedes IS NULL OR kind = 'decision'),
  CHECK ((question IS NOT NULL) = (kind = 'answer')),
  CHECK ((plan_id IS NOT NULL) = (kind = 'plan')),
  CHECK ((plan_id IS NULL) = (plan_machine IS NULL) AND (plan_id IS NULL) = (plan_repo IS NULL))
) STRICT;

-- ---------- Plans ----------
CREATE TABLE plans (
  id            TEXT PRIMARY KEY CHECK (length(id) BETWEEN 2 AND 24),
  context       TEXT NOT NULL REFERENCES contexts(id),
  title         TEXT NOT NULL CHECK (length(title) >= 1),
  jira          TEXT,
  repo          TEXT NOT NULL,
  base          TEXT NOT NULL,
  lead_branch   TEXT NOT NULL UNIQUE CHECK (lead_branch = 'cpd/' || id),
  lead_worktree TEXT,
  phase         TEXT NOT NULL CHECK (phase IN ('grounding', 'plan', 'dispatch', 'pr', 'done')),
  lead_session  TEXT NOT NULL REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
  pr_number     INTEGER,
  pr_url        TEXT,
  pr_opened_at  TEXT,
  pr_seen       TEXT NOT NULL DEFAULT '{"checks":{},"reviewIds":[],"commentIds":[]}' CHECK (json_valid(pr_seen)),
  pr_polled_at  TEXT,
  all_merged_at TEXT,
  created_at    TEXT NOT NULL,
  locked_at     TEXT,
  done_at       TEXT,
  CHECK ((phase IN ('grounding', 'plan')) = (lead_worktree IS NULL)),
  CHECK ((phase IN ('grounding', 'plan')) = (locked_at IS NULL)),
  CHECK ((phase IN ('pr', 'done')) = (pr_number IS NOT NULL)),
  CHECK ((pr_number IS NULL) = (pr_url IS NULL) AND (pr_number IS NULL) = (pr_opened_at IS NULL)),
  CHECK ((phase = 'done') = (done_at IS NOT NULL))
) STRICT;

CREATE TABLE plan_reviews (
  plan         TEXT NOT NULL REFERENCES plans(id),
  id           TEXT NOT NULL,
  number       INTEGER NOT NULL CHECK (number >= 1),
  fingerprint  TEXT NOT NULL CHECK (length(fingerprint) = 64),
  status       TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'rejected')),
  feedback     TEXT,
  requested_at TEXT NOT NULL,
  decided_at   TEXT,
  PRIMARY KEY (plan, id),
  UNIQUE (plan, number),
  CHECK (id = 'r' || number),
  CHECK ((status = 'pending') = (decided_at IS NULL))
) STRICT;

-- ---------- Jobs ----------
CREATE TABLE jobs (
  seq                 INTEGER PRIMARY KEY AUTOINCREMENT,
  plan                TEXT NOT NULL REFERENCES plans(id),
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
  CHECK (status <> 'draft' OR fingerprint IS NULL),
  CHECK (status IN ('draft', 'cancelled') OR fingerprint IS NOT NULL),
  CHECK ((status = 'merged') = (merged_sha IS NOT NULL)),
  CHECK ((status = 'failed') = (failed_cause IS NOT NULL)),
  CHECK ((cancel_reason IS NULL) = (cancel_requested_at IS NULL)),
  CHECK (status <> 'cancelled' OR cancel_reason IS NOT NULL)
) STRICT;

CREATE TABLE dependencies (
  plan  TEXT NOT NULL,
  job   TEXT NOT NULL,
  needs TEXT NOT NULL,
  PRIMARY KEY (plan, job, needs),
  FOREIGN KEY (plan, job)   REFERENCES jobs(plan, key),
  FOREIGN KEY (plan, needs) REFERENCES jobs(plan, key),
  CHECK (job <> needs)
) STRICT;

-- ---------- Attempts ----------
CREATE TABLE attempts (
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
                                         'cas_lost', 'dirty_target', 'tampered')),
  branch           TEXT NOT NULL CHECK (branch = 'cpd/' || plan || '--' || id),
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
CREATE UNIQUE INDEX attempts_one_active ON attempts(plan, job) WHERE status <> 'ended';
CREATE INDEX attempts_active ON attempts(status) WHERE status <> 'ended';

-- ---------- Escalations ----------
CREATE TABLE escalations (
  plan        TEXT NOT NULL,
  id          TEXT NOT NULL,
  attempt     TEXT NOT NULL,
  number      INTEGER NOT NULL CHECK (number >= 1),
  question    TEXT NOT NULL CHECK (length(question) >= 1),
  status      TEXT NOT NULL CHECK (status IN ('open', 'answered', 'expired')),
  answer      TEXT,
  asker_pid   INTEGER NOT NULL,
  asked_at    TEXT NOT NULL,
  answered_at TEXT,
  expired_at  TEXT,
  PRIMARY KEY (plan, id),
  UNIQUE (plan, attempt, number),
  FOREIGN KEY (plan, attempt) REFERENCES attempts(plan, id),
  CHECK (id = attempt || '-q' || number),
  CHECK ((status = 'answered') = (answer IS NOT NULL)),
  CHECK ((status = 'answered') = (answered_at IS NOT NULL)),
  CHECK ((status = 'expired') = (expired_at IS NOT NULL))
) STRICT;

-- ---------- Sessions ----------
CREATE TABLE sessions (
  id          TEXT PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 128),
  role        TEXT NOT NULL CHECK (role IN ('lead', 'worker')),
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
  FOREIGN KEY (plan, attempt) REFERENCES attempts(plan, id) DEFERRABLE INITIALLY DEFERRED,
  CHECK ((role = 'lead') = (attempt IS NULL)),
  CHECK (role = 'lead' OR plan IS NOT NULL),
  CHECK ((state = 'blocked') = (state_label IS NOT NULL))
) STRICT;
CREATE INDEX sessions_plan ON sessions(plan);

-- ---------- Events ----------
CREATE TABLE events (
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
CREATE INDEX events_plan ON events(plan, id);
CREATE INDEX events_context ON events(context, id);
CREATE INDEX events_session ON events(session, id);

-- ---------- Tier catalog cache (§9) ----------
CREATE TABLE tier_catalog (
  pi_version TEXT NOT NULL,
  letter     TEXT NOT NULL CHECK (letter IN ('O', 'S', 'H')),
  model      TEXT NOT NULL,
  efforts    TEXT NOT NULL CHECK (json_valid(efforts) AND json_type(efforts) = 'array'),
  read_at    TEXT NOT NULL,
  PRIMARY KEY (pi_version, letter, model)
) STRICT;

CREATE TRIGGER dependencies_no_delete BEFORE DELETE ON dependencies
WHEN (SELECT status FROM jobs WHERE plan = OLD.plan AND key = OLD.job) <> 'draft'
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;
CREATE TRIGGER dependencies_hist_del AFTER DELETE ON dependencies BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'dependencies',
    json_array(OLD.plan, OLD.job, OLD.needs), 'delete',
    json_object('plan', OLD.plan, 'job', OLD.job, 'needs', OLD.needs), NULL);
END;

CREATE TRIGGER jobs_locked BEFORE UPDATE OF title, tier, goal, context, touches, acceptance, out_of_scope, plan, key ON jobs
WHEN OLD.status <> 'draft'
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;

CREATE TRIGGER dependencies_locked BEFORE INSERT ON dependencies
WHEN (SELECT status FROM jobs WHERE plan = NEW.plan AND key = NEW.job) <> 'draft'
 AND (SELECT created_txn FROM jobs WHERE plan = NEW.plan AND key = NEW.job) <> cpd_txn()
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;

CREATE TRIGGER plans_phase BEFORE UPDATE OF phase ON plans
WHEN OLD.phase <> NEW.phase AND (OLD.phase || '>' || NEW.phase) NOT IN
  ('grounding>plan', 'plan>dispatch', 'dispatch>pr', 'pr>done')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER jobs_status BEFORE UPDATE OF status ON jobs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('draft>queued', 'queued>running', 'running>merged',
   'running>queued', 'running>failed', 'failed>queued',
   'draft>cancelled', 'queued>cancelled', 'failed>cancelled', 'running>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER attempts_status BEFORE UPDATE OF status ON attempts
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('starting>running', 'starting>ended', 'running>merging', 'running>ended', 'merging>ended')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER attempts_ended BEFORE UPDATE ON attempts
WHEN OLD.status = 'ended'
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;   -- metrics are written in the transaction that ends the Attempt

CREATE TRIGGER escalations_status BEFORE UPDATE OF status ON escalations
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN ('open>answered', 'open>expired')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER context_records_immutable BEFORE UPDATE ON context_records
BEGIN SELECT RAISE(ABORT, 'append_only: context_records'); END;

CREATE TRIGGER txns_own BEFORE INSERT ON txns WHEN NEW.id IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'txns.id must be cpd_txn()'); END;
CREATE TRIGGER events_own BEFORE INSERT ON events WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'events.txn must be cpd_txn()'); END;
CREATE TRIGGER history_own BEFORE INSERT ON history WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'history.txn must be cpd_txn()'); END;
CREATE TRIGGER txns_no_update    BEFORE UPDATE ON txns    BEGIN SELECT RAISE(ABORT, 'append_only: txns'); END;
CREATE TRIGGER txns_no_delete    BEFORE DELETE ON txns    BEGIN SELECT RAISE(ABORT, 'append_only: txns'); END;
CREATE TRIGGER history_no_update BEFORE UPDATE ON history BEGIN SELECT RAISE(ABORT, 'append_only: history'); END;
CREATE TRIGGER history_no_delete BEFORE DELETE ON history BEGIN SELECT RAISE(ABORT, 'append_only: history'); END;
CREATE TRIGGER events_no_update  BEFORE UPDATE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
CREATE TRIGGER events_no_delete  BEFORE DELETE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
