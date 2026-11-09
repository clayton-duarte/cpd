-- cpd-migration: breaking after 125
-- The Inbox kind `bug` (the owner's wake and the User's breach indicator, contract §2.2 "Inbox item",
-- "Bug record"; air-d104, air-d105, air-n120) and the Event kind `bug-breach` (contract §5.2, §3.7 E25).
-- `inbox_items` is rebuilt with `bug` in its kind check, as `pr-closed-kind` rebuilt it, from the shape
-- `refusal-alert-kind` left it (`head_oid`, `reviewed_oid`, and `refusal-alert`); SQLite cannot alter a CHECK constraint.
CREATE TABLE inbox_items_new (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('dialog', 'plan-review', 'unread', 'job-stuck', 'engine-error', 'question', 'pr-failing', 'pr-changes', 'pr-ready', 'review-requested', 'curate-retire', 'curate-docs', 'loop-guard', 'deploy', 'models-changed', 'drafts-parked', 'pr-closed', 'refusal-alert', 'bug')),
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
INSERT INTO event_kinds (name) VALUES ('bug-breach');
