-- cpd-migration: breaking after 146
-- Preserve Plan rows and history while allowing an unlocked shaping-origin Plan to finish.
-- The migration runner restores the generated history triggers after this rebuild.

CREATE TABLE plans_new (
  id            TEXT PRIMARY KEY CHECK (length(id) BETWEEN 2 AND 24),
  context       TEXT NOT NULL REFERENCES contexts(id),
  title         TEXT NOT NULL CHECK (length(title) >= 1),
  jira          TEXT,
  phase         TEXT NOT NULL CHECK (phase IN ('shaping', 'grounding', 'plan', 'dispatch', 'deploy', 'done')),
  lead_session  TEXT REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
  created_at    TEXT NOT NULL,
  locked_at     TEXT,
  done_at       TEXT,
  archived_at   TEXT,
  outline       TEXT CHECK (outline IS NULL OR (json_valid(outline) AND json_type(outline) = 'object')),
  deleted_at    TEXT,
  pr_subject    TEXT,
  changes_requested_at TEXT,
  reviewed_repo   TEXT CHECK (reviewed_repo IS NULL OR reviewed_repo LIKE '_%/_%'),
  reviewed_number INTEGER CHECK (reviewed_number IS NULL OR reviewed_number >= 1),
  reviewed_title  TEXT,
  shaping_origin INTEGER NOT NULL DEFAULT 0 CHECK (shaping_origin IN (0, 1)),
  CHECK ((phase IN ('grounding', 'shaping') AND locked_at IS NULL) OR (phase IN ('dispatch', 'deploy') AND locked_at IS NOT NULL) OR phase = 'plan' OR (phase = 'done' AND ((lead_session IS NULL) = (locked_at IS NULL) OR (shaping_origin = 1 AND lead_session IS NOT NULL AND locked_at IS NULL)))),
  CHECK ((phase = 'done') = (done_at IS NOT NULL)),
  CHECK (lead_session IS NOT NULL OR phase IN ('shaping', 'done'))
) STRICT;
INSERT INTO plans_new (id, context, title, jira, phase, lead_session, created_at, locked_at, done_at, archived_at, outline,
                       deleted_at, pr_subject, changes_requested_at, reviewed_repo, reviewed_number, reviewed_title, shaping_origin)
  SELECT id, context, title, jira, phase, lead_session, created_at, locked_at, done_at, archived_at, outline,
         deleted_at, pr_subject, changes_requested_at, reviewed_repo, reviewed_number, reviewed_title, phase = 'shaping'
  FROM plans;
DROP TABLE plans;
ALTER TABLE plans_new RENAME TO plans;

CREATE TRIGGER plans_deleted_once BEFORE UPDATE OF deleted_at ON plans
WHEN OLD.deleted_at IS NOT NULL AND NEW.deleted_at IS NOT OLD.deleted_at
BEGIN SELECT RAISE(ABORT, 'append_only: plans.deleted_at'); END;

CREATE TRIGGER plans_done_no_pending_review BEFORE UPDATE OF phase ON plans
WHEN NEW.phase = 'done' AND OLD.phase <> 'done'
 AND EXISTS (SELECT 1 FROM plan_reviews WHERE plan = NEW.id AND status = 'pending')
BEGIN SELECT RAISE(ABORT, 'review_pending'); END;

CREATE TRIGGER plans_phase BEFORE UPDATE OF phase ON plans
WHEN OLD.phase <> NEW.phase AND (OLD.phase || '>' || NEW.phase) NOT IN
  ('grounding>plan', 'plan>dispatch', 'dispatch>deploy', 'deploy>dispatch', 'done>dispatch', 'done>deploy',
   'dispatch>plan', 'deploy>plan', 'done>plan', 'shaping>done', 'done>shaping')
  AND NOT (OLD.phase = 'grounding' AND NEW.phase = 'done' AND NEW.context = 'codereview')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER plans_shaping_origin_immutable BEFORE UPDATE OF shaping_origin ON plans
WHEN NEW.shaping_origin IS NOT OLD.shaping_origin
BEGIN SELECT RAISE(ABORT, 'append_only: plans.shaping_origin'); END;
