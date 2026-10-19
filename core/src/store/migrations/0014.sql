-- Migration 14 (contract §4.5, stub drafts, hs-d241, R1): `jobs.tier`, `goal`,
-- `context`, `touches`, `acceptance` and `out_of_scope` become nullable, but only
-- while the Job is a `draft` or `cancelled` (a stub can be cancelled). SQLite
-- cannot drop a NOT NULL or alter a check, so `jobs` is rebuilt as migration 4
-- rebuilt it: the new table, every row copied with its `seq`, the old one
-- dropped, the new one renamed, then the triggers of §4.4 on `jobs`. schema.rs
-- runs this text with foreign keys off and legacy_alter_table on, inside the
-- migrating transaction, and generates the history triggers and `jobs_no_delete`
-- of `jobs` after it. `PRAGMA foreign_key_check` is the caller's, before the
-- commit. Every existing row has every field, so no row changes; `jobs` is never
-- deleted from, so `seq` goes on from the largest copied.
-- Frozen: the schema hash of version 14 covers this text. Change the schema
-- only with a new migration.

CREATE TABLE jobs_new (
  seq                 INTEGER PRIMARY KEY AUTOINCREMENT,
  plan                TEXT NOT NULL REFERENCES plans(id),
  pr                  TEXT NOT NULL,          -- R34: the Job's PR (A7)
  review              INTEGER NOT NULL DEFAULT 0 CHECK (review IN (0, 1)),  -- B2: a review Job (added by migration 4)
  key                 TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 32),
  title               TEXT NOT NULL CHECK (length(title) >= 1),
  tier                TEXT CHECK (tier IS NULL OR (length(tier) = 2 AND substr(tier, 1, 1) IN ('O', 'S', 'H') AND substr(tier, 2, 1) BETWEEN '0' AND '5')),  -- null only while draft (migration 0014)
  goal                TEXT CHECK (goal IS NULL OR length(goal) >= 1),
  context             TEXT,
  touches             TEXT CHECK (touches IS NULL OR (json_valid(touches) AND json_type(touches) = 'array' AND json_array_length(touches) >= 1)),
  acceptance          TEXT CHECK (acceptance IS NULL OR (json_valid(acceptance) AND json_type(acceptance) = 'array' AND json_array_length(acceptance) >= 1)),
  out_of_scope        TEXT CHECK (out_of_scope IS NULL OR (json_valid(out_of_scope) AND json_type(out_of_scope) = 'array')),
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
  CHECK (status IN ('draft', 'cancelled') OR (tier IS NOT NULL AND goal IS NOT NULL AND context IS NOT NULL AND touches IS NOT NULL AND acceptance IS NOT NULL AND out_of_scope IS NOT NULL)),  -- stubs only as drafts, or cancelled as such (hs-d241)
  CHECK ((goal IS NULL) = (context IS NULL) AND (goal IS NULL) = (touches IS NULL) AND (goal IS NULL) = (acceptance IS NULL) AND (goal IS NULL) = (out_of_scope IS NULL)),  -- the Spec is all or nothing
  CHECK ((status = 'merged') = (merged_sha IS NOT NULL)),
  CHECK ((status = 'failed') = (failed_cause IS NOT NULL)),
  CHECK ((cancel_reason IS NULL) = (cancel_requested_at IS NULL)),
  CHECK (status <> 'cancelled' OR cancel_reason IS NOT NULL)
) STRICT;
INSERT INTO jobs_new (seq, plan, pr, review, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                      status, appended, fingerprint, created_txn, retry_tier, cancel_reason, cancel_requested_at,
                      failed_cause, merged_sha, created_at)
  SELECT seq, plan, pr, review, key, title, tier, goal, context, touches, acceptance, out_of_scope,
         status, appended, fingerprint, created_txn, retry_tier, cancel_reason, cancel_requested_at,
         failed_cause, merged_sha, created_at
  FROM jobs ORDER BY seq;
DROP TABLE jobs;
ALTER TABLE jobs_new RENAME TO jobs;

-- The triggers of `jobs` (§4.4), written again: they went with the old table.
CREATE TRIGGER jobs_locked BEFORE UPDATE OF title, tier, goal, context, touches, acceptance, out_of_scope, plan, key, pr, review ON jobs
WHEN OLD.status <> 'draft'
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;

CREATE TRIGGER jobs_pr_dependencies BEFORE UPDATE OF pr ON jobs
WHEN OLD.pr <> NEW.pr AND EXISTS (SELECT 1 FROM dependencies WHERE plan = NEW.plan AND (job = NEW.key OR needs = NEW.key))
BEGIN SELECT RAISE(ABORT, 'needs_other_pr'); END;

CREATE TRIGGER jobs_status BEFORE UPDATE OF status ON jobs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('draft>queued', 'queued>running', 'running>merged',
   'running>queued', 'running>failed', 'failed>queued',
   'draft>cancelled', 'queued>cancelled', 'failed>cancelled', 'running>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
