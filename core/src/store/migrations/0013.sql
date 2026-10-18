-- Migration 13 (contract §4.5, the PR poll's Inbox kinds, hs-d165, D1, D4):
-- `pull_requests.authored` and `review_requested`, and the history triggers of
-- `pull_requests` dropped and recreated so they list every column; then
-- `inbox_items` rebuilt with the four pull request kinds in its check. schema.rs
-- runs this text with foreign keys off and legacy_alter_table on, then
-- generates `pull_requests_hist_ins` and `pull_requests_hist_upd` from that
-- table's columns, and the history triggers and `inbox_items_no_delete` of
-- `inbox_items`. The `ALTER`s write no
-- history row: every existing row came from the `author:@me` search, so the
-- defaults give it `authored` 1 and `review_requested` 0. `pull_requests_owned`
-- is unchanged and the table still gets no `_no_delete`.
-- Frozen: the schema hash of version 13 covers this text. Change the schema
-- only with a new migration.

ALTER TABLE pull_requests ADD COLUMN authored INTEGER NOT NULL DEFAULT 1 CHECK (authored IN (0, 1));
ALTER TABLE pull_requests ADD COLUMN review_requested INTEGER NOT NULL DEFAULT 0 CHECK (review_requested IN (0, 1));

DROP TRIGGER pull_requests_hist_ins;
DROP TRIGGER pull_requests_hist_upd;
DROP TRIGGER pull_requests_hist_del;

CREATE TRIGGER pull_requests_hist_del AFTER DELETE ON pull_requests BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'pull_requests',
    json_array(OLD.id), 'delete',
    json_object('id', OLD.id, 'repo', OLD.repo, 'number', OLD.number, 'url', OLD.url, 'title', OLD.title,
                'draft', OLD.draft, 'state', OLD.state, 'review_decision', OLD.review_decision,
                'checks', OLD.checks, 'created_at', OLD.created_at, 'merged_at', OLD.merged_at,
                'changed_at', OLD.changed_at, 'mergeable', OLD.mergeable, 'merge_state', OLD.merge_state,
                'authored', OLD.authored, 'review_requested', OLD.review_requested), NULL);
END;

-- ---------- inbox_items, rebuilt ----------
-- SQLite cannot alter a check: create the new table, copy every row with its id,
-- drop the old one (its index and triggers go with it), rename, and recreate the
-- indexes. The copy writes no history row: the new table has no trigger yet.
CREATE TABLE inbox_items_new (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('dialog', 'plan-review', 'unread', 'job-stuck', 'engine-error', 'question', 'pr-failing', 'pr-changes', 'pr-ready', 'review-requested')),
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
