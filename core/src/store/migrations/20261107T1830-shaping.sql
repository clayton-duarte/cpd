-- cpd-migration: breaking after 120
-- shaping (air-d19, air-d22; contract §4.4, §4.5): the Lead-less shaping Plan.
--   * `plans` is rebuilt (`ALTER TABLE` cannot change a CHECK; the runner applies a migration with
--     foreign keys off and `legacy_alter_table` on, as `0008-phase-deploy` and `replan-phase` relied on):
--     `phase`'s CHECK also lists `'shaping'`, `lead_session` is nullable, and the table's CHECK list
--     gains the two `lead_session` rules (`lead_session IS NOT NULL OR phase IN ('shaping', 'done')`,
--     `phase <> 'shaping' OR lead_session IS NULL`) and the amended `locked_at` rule (a Lead-less done
--     Plan has none). Every row and column is copied with the primary key kept.
--   * `plans_phase` also allows `shaping>done` and `done>shaping`, and no other transition for `shaping`.
--   * `plans_deleted_once` and `plans_done_no_pending_review` are recreated; the history triggers are the reconcile's.
-- Breaking: a rebuild and a dropped trigger, and an old binary would refuse a `shaping` Plan.

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
  CHECK ((phase IN ('grounding', 'shaping') AND locked_at IS NULL) OR (phase IN ('dispatch', 'deploy') AND locked_at IS NOT NULL) OR phase = 'plan' OR (phase = 'done' AND (lead_session IS NULL) = (locked_at IS NULL))),
  CHECK ((phase = 'done') = (done_at IS NOT NULL)),
  CHECK (lead_session IS NOT NULL OR phase IN ('shaping', 'done')),
  CHECK (phase <> 'shaping' OR lead_session IS NULL)
) STRICT;
INSERT INTO plans_new (id, context, title, jira, phase, lead_session, created_at, locked_at, done_at, archived_at, outline,
                       deleted_at, pr_subject, changes_requested_at, reviewed_repo, reviewed_number, reviewed_title)
  SELECT id, context, title, jira, phase, lead_session, created_at, locked_at, done_at, archived_at, outline,
         deleted_at, pr_subject, changes_requested_at, reviewed_repo, reviewed_number, reviewed_title
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
  ('grounding>plan', 'plan>dispatch', 'dispatch>deploy', 'deploy>done', 'deploy>dispatch', 'done>dispatch', 'done>deploy',
   'dispatch>plan', 'deploy>plan', 'done>plan', 'shaping>done', 'done>shaping')
  AND NOT (OLD.phase = 'grounding' AND NEW.phase = 'done' AND NEW.context = 'codereview')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
