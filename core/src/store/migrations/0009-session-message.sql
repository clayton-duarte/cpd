-- Migration 9 (contract §4.5, Plan manager B2, hs-d92 A4, hs-d133, B2-A3): the
-- Event kind `session-message` (`cpd session send`). `ALTER TABLE` cannot change
-- a CHECK, so `events` is rebuilt as migrations 3 and 4 rebuilt it, with every
-- earlier kind kept and every row copied with its id; the caller runs this
-- inside the migrating transaction with foreign keys off and
-- `legacy_alter_table` on, and checks `foreign_key_check` before the commit.

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
            'session-message',
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
-- Ids never go back: the new table's sequence is at least the old one's (as migration 3).
UPDATE sqlite_sequence SET seq = max(seq, coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'events'), 0))
  WHERE name = 'events_new';
DROP TABLE events;
ALTER TABLE events_new RENAME TO events;
CREATE INDEX events_plan ON events(plan, id);
CREATE INDEX events_context ON events(context, id);
CREATE INDEX events_session ON events(session, id);
-- After the copy above: the copied Events carry their old txn.
CREATE TRIGGER events_own BEFORE INSERT ON events WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'events.txn must be cpd_txn()'); END;
CREATE TRIGGER events_no_update  BEFORE UPDATE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
CREATE TRIGGER events_no_delete  BEFORE DELETE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
