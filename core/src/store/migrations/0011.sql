-- Migration 11 (contract §4.5, the Inbox, hs-d168): the table `inbox_items`
-- with its partial unique index, and `events` rebuilt with the four `inbox-*`
-- kinds in its check (as migration 3 did). schema.rs runs it with foreign keys
-- off and legacy_alter_table on, and generates the history triggers and
-- `inbox_items_no_delete` of `inbox_items` after it.
-- Frozen: the schema hash of its version covers this text. Change the schema
-- only with a new migration.

-- ---------- Inbox items (§2.2) ----------
CREATE TABLE inbox_items (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('dialog', 'plan-review', 'unread', 'job-stuck', 'engine-error', 'question')),
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
CREATE UNIQUE INDEX inbox_items_open ON inbox_items(kind, subject, urgency) WHERE closed_at IS NULL;
CREATE INDEX inbox_items_closed ON inbox_items(closed_at);

-- ---------- events, rebuilt ----------
-- SQLite cannot alter a check: create the new table, copy every row with its id,
-- drop the old one, rename, and recreate its indexes and triggers.
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
            'context-renamed', 'plan-renamed',
            'context-archived', 'plan-archived', 'session-archived',
            'inbox-opened', 'inbox-closed', 'inbox-snoozed', 'inbox-dismissed',
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
  SELECT id, txn, at, kind, context, plan, job, attempt, session, text, data FROM events ORDER BY id;
-- Ids never go back: the new table's sequence is at least the old one's.
UPDATE sqlite_sequence SET seq = max(seq, coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'events'), 0))
  WHERE name = 'events_new';
DROP TABLE events;
ALTER TABLE events_new RENAME TO events;
CREATE INDEX events_plan ON events(plan, id);
CREATE INDEX events_context ON events(context, id);
CREATE INDEX events_session ON events(session, id);
CREATE TRIGGER events_own BEFORE INSERT ON events WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'events.txn must be cpd_txn()'); END;
CREATE TRIGGER events_no_update  BEFORE UPDATE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
CREATE TRIGGER events_no_delete  BEFORE DELETE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
