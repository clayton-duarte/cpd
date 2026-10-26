-- cpd-migration: breaking after 54
-- pro-d58: add the Inbox tier `blue` for external support (review-requested); `inbox_items` is rebuilt as `pr-adopt` rebuilt it.
CREATE TABLE inbox_items_new (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('dialog', 'plan-review', 'review-waiting', 'unread', 'job-stuck', 'engine-error', 'question', 'pr-failing', 'pr-changes', 'pr-ready', 'review-requested', 'curate-retire', 'curate-docs', 'loop-guard')),
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
  CHECK (plan IS NULL OR context IS NOT NULL),
  CHECK (job IS NULL OR plan IS NOT NULL)
) STRICT;
INSERT INTO inbox_items_new (id, kind, urgency, subject, context, plan, job, session, record, event, label, url, opened_at, closed_at, snoozed_until, dismissed_at)
  SELECT id, kind, CASE WHEN kind = 'review-requested' THEN 'blue' ELSE urgency END, subject, context, plan, job, session, record, event, label, url, opened_at, closed_at, snoozed_until, dismissed_at FROM inbox_items ORDER BY id;
DROP TABLE inbox_items;
ALTER TABLE inbox_items_new RENAME TO inbox_items;
CREATE UNIQUE INDEX inbox_items_open ON inbox_items(kind, subject, urgency) WHERE closed_at IS NULL;
CREATE INDEX inbox_items_closed ON inbox_items(closed_at);
