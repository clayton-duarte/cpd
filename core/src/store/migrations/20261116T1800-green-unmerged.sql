-- cpd-migration: breaking after 144
CREATE TABLE inbox_items_new (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('dialog', 'plan-review', 'unread', 'job-stuck', 'engine-error', 'question', 'pr-failing', 'pr-changes', 'pr-ready', 'review-requested', 'curate-retire', 'curate-docs', 'loop-guard', 'deploy', 'models-changed', 'drafts-parked', 'pr-closed', 'refusal-alert', 'bug', 'green-unmerged')),
  urgency       TEXT NOT NULL CHECK (urgency IN ('red', 'yellow', 'magenta', 'blue', 'cyan', 'white')),
  subject       TEXT NOT NULL CHECK (length(subject) >= 1),
  context       TEXT REFERENCES contexts(id),
  plan          TEXT REFERENCES plans(id),
  job           TEXT,
  session       TEXT REFERENCES sessions(id),
  record        TEXT,
  event         INTEGER REFERENCES events(id),
  label         TEXT NOT NULL CHECK (length(label) >= 1),
  url           TEXT,
  opened_at     TEXT NOT NULL,
  closed_at     TEXT,
  snoozed_until TEXT,
  dismissed_at  TEXT,
  claimed_at    TEXT CHECK (claimed_at IS NULL OR kind = 'review-requested'),
  head_oid      TEXT,
  reviewed_oid  TEXT,
  CHECK (plan IS NULL OR context IS NOT NULL),
  CHECK (job IS NULL OR plan IS NOT NULL)
) STRICT;
INSERT INTO inbox_items_new (id, kind, urgency, subject, context, plan, job, session, record, event, label, url, opened_at, closed_at, snoozed_until, dismissed_at, claimed_at, head_oid, reviewed_oid)
  SELECT id, kind, urgency, subject, context, plan, job, session, record, event, label, url, opened_at, closed_at, snoozed_until, dismissed_at, claimed_at, head_oid, reviewed_oid FROM inbox_items ORDER BY id;
DROP TABLE inbox_items;
ALTER TABLE inbox_items_new RENAME TO inbox_items;
CREATE UNIQUE INDEX inbox_items_open ON inbox_items(kind, subject, urgency) WHERE closed_at IS NULL;
CREATE INDEX inbox_items_closed ON inbox_items(closed_at);


CREATE TABLE green_unmerged_periods (
  pull_request INTEGER PRIMARY KEY REFERENCES pull_requests(id),
  since INTEGER NOT NULL,
  alerted INTEGER NOT NULL CHECK (alerted IN (0, 1)),
  manager_told INTEGER NOT NULL DEFAULT 0 CHECK (manager_told IN (0, 1))
) STRICT;

-- A non-green observation breaks the period even if it returns to green before the next tick.
CREATE TRIGGER green_unmerged_pr_reset AFTER UPDATE OF checks, state ON pull_requests
WHEN NEW.state != 'open' OR NEW.checks != 'passed'
BEGIN
  DELETE FROM green_unmerged_periods WHERE pull_request = NEW.id;
END;
CREATE TRIGGER green_unmerged_workflow_reset AFTER UPDATE OF status, pull_request ON workflows
WHEN OLD.pull_request IS NOT NULL AND NOT EXISTS (
  SELECT 1 FROM workflows WHERE pull_request = OLD.pull_request AND status = 'open'
)
BEGIN
  DELETE FROM green_unmerged_periods WHERE pull_request = OLD.pull_request;
END;
