-- cpd-migration: breaking after 29
-- The Inbox kind `review-waiting` (hs-d496, contract §4.4, §4.5): `inbox_items` rebuilt with the kind in its check, as
-- `0013-pr-inbox` did. Every row keeps its id; the copy writes no history row, and the runner's reconcile generates the
-- history triggers and `inbox_items_no_delete` of the new table.
CREATE TABLE inbox_items_new (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('dialog', 'plan-review', 'review-waiting', 'unread', 'job-stuck', 'engine-error', 'question', 'pr-failing', 'pr-changes', 'pr-ready', 'review-requested')),
  urgency       TEXT NOT NULL CHECK (urgency IN ('red', 'yellow', 'magenta', 'white')),
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
  CHECK (plan IS NULL OR context IS NOT NULL),
  CHECK (job IS NULL OR plan IS NOT NULL)
) STRICT;
INSERT INTO inbox_items_new (id, kind, urgency, subject, context, plan, job, session, record, event, label, url, opened_at, closed_at, snoozed_until, dismissed_at)
  SELECT id, kind, urgency, subject, context, plan, job, session, record, event, label, url, opened_at, closed_at, snoozed_until, dismissed_at FROM inbox_items ORDER BY id;
DROP TABLE inbox_items;
ALTER TABLE inbox_items_new RENAME TO inbox_items;
CREATE UNIQUE INDEX inbox_items_open ON inbox_items(kind, subject, urgency) WHERE closed_at IS NULL;
CREATE INDEX inbox_items_closed ON inbox_items(closed_at);
