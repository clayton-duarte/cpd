-- Migration 8 (contract §4.5, Plan manager B1, hs-d76, hs-d131): the Plan phase
-- `pr` is called `deploy`. `ALTER TABLE` cannot change a CHECK, so `plans` is
-- rebuilt as migration 4 rebuilt it; the caller runs this inside the migrating
-- transaction with foreign keys off and `legacy_alter_table` on, and generates
-- the history triggers of `plans` afterwards.

CREATE TABLE plans_new (
  id            TEXT PRIMARY KEY CHECK (length(id) BETWEEN 2 AND 24),
  context       TEXT NOT NULL REFERENCES contexts(id),
  title         TEXT NOT NULL CHECK (length(title) >= 1),
  jira          TEXT,
  phase         TEXT NOT NULL CHECK (phase IN ('grounding', 'plan', 'dispatch', 'deploy', 'done')),
  lead_session  TEXT NOT NULL REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
  created_at    TEXT NOT NULL,
  locked_at     TEXT,
  done_at       TEXT,
  archived_at   TEXT,                          -- migration 3 (archive)
  CHECK ((phase IN ('grounding', 'plan')) = (locked_at IS NULL)),
  CHECK ((phase = 'done') = (done_at IS NOT NULL))
) STRICT;
INSERT INTO plans_new (id, context, title, jira, phase, lead_session, created_at, locked_at, done_at, archived_at)
  SELECT id, context, title, jira, CASE phase WHEN 'pr' THEN 'deploy' ELSE phase END,
         lead_session, created_at, locked_at, done_at, archived_at FROM plans;
DROP TABLE plans;
ALTER TABLE plans_new RENAME TO plans;

CREATE TRIGGER plans_phase BEFORE UPDATE OF phase ON plans
WHEN OLD.phase <> NEW.phase AND (OLD.phase || '>' || NEW.phase) NOT IN
  ('grounding>plan', 'plan>dispatch', 'dispatch>deploy', 'deploy>done', 'deploy>dispatch', 'done>dispatch')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

-- The `from` and `to` of the `plan-phase` Events. Event text and kinds stay.
-- Events are append-only: the guards go for the rewrite and come back.
DROP TRIGGER events_no_update;
DROP TRIGGER events_no_delete;
UPDATE events SET data = json_set(data, '$.from', 'deploy')
  WHERE kind = 'plan-phase' AND json_extract(data, '$.from') = 'pr';
UPDATE events SET data = json_set(data, '$.to', 'deploy')
  WHERE kind = 'plan-phase' AND json_extract(data, '$.to') = 'pr';
CREATE TRIGGER events_no_update  BEFORE UPDATE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
CREATE TRIGGER events_no_delete  BEFORE DELETE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
