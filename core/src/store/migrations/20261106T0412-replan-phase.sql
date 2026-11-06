-- cpd-migration: breaking after 114
-- replan-phase (pro-d429, pro-d433): a locked Plan may go back to phase `plan` and keep `locked_at`.
--   * `plans` is rebuilt (`ALTER TABLE` cannot change a CHECK; the runner applies a migration with foreign keys off and
--     `legacy_alter_table` on, as `0008-phase-deploy` relied on): `grounding` needs `locked_at` NULL; `dispatch`, `deploy` and
--     `done` need it set; `plan` allows either. `(phase = 'done') = (done_at IS NOT NULL)` is unchanged. Every row is copied;
--     the history triggers and `plans_no_delete` of `plans` are the reconcile's.
--   * `plans_phase` also allows `dispatch>plan`, `deploy>plan` and `done>plan`.
--   * the triggers that read "before the lock" as `phase IN ('grounding', 'plan')` test `locked_at IS NULL` instead:
--     `workflows_locked`, `workflow_needs_locked` and `workflow_needs_no_delete`.
-- Breaking: a rebuild and a dropped trigger, and an old binary would refuse a `plan` Plan with `locked_at` set.

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
  archived_at   TEXT,
  outline       TEXT CHECK (outline IS NULL OR (json_valid(outline) AND json_type(outline) = 'object')),
  deleted_at    TEXT,
  pr_subject    TEXT,
  changes_requested_at TEXT,
  reviewed_repo   TEXT CHECK (reviewed_repo IS NULL OR reviewed_repo LIKE '_%/_%'),
  reviewed_number INTEGER CHECK (reviewed_number IS NULL OR reviewed_number >= 1),
  reviewed_title  TEXT,
  CHECK ((phase = 'grounding' AND locked_at IS NULL) OR (phase IN ('dispatch', 'deploy', 'done') AND locked_at IS NOT NULL) OR phase = 'plan'),
  CHECK ((phase = 'done') = (done_at IS NOT NULL))
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
   'dispatch>plan', 'deploy>plan', 'done>plan')
  AND NOT (OLD.phase = 'grounding' AND NEW.phase = 'done' AND NEW.context = 'codereview')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

DROP TRIGGER workflows_locked;
CREATE TRIGGER workflows_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON workflows
WHEN (SELECT locked_at FROM plans WHERE id = OLD.plan) IS NOT NULL
 AND NOT (OLD.status IN ('waiting', 'dispatch', 'open')
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.lead_branch IS OLD.lead_branch
          AND NEW.base IS NOT OLD.base)
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

DROP TRIGGER workflow_needs_locked;
CREATE TRIGGER workflow_needs_locked BEFORE INSERT ON workflow_needs
WHEN (SELECT locked_at FROM plans WHERE id = NEW.plan) IS NOT NULL
 AND (SELECT created_txn FROM workflows WHERE plan = NEW.plan AND key = NEW.workflow) <> cpd_txn()
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

DROP TRIGGER workflow_needs_no_delete;
CREATE TRIGGER workflow_needs_no_delete BEFORE DELETE ON workflow_needs
WHEN (SELECT locked_at FROM plans WHERE id = OLD.plan) IS NOT NULL
 AND NOT ((SELECT deleted_at FROM plans WHERE id = OLD.plan) IS NOT NULL)
 AND NOT EXISTS (SELECT 1 FROM workflows r, workflows q WHERE r.plan = OLD.plan AND r.key = OLD.workflow AND r.status = 'waiting' AND q.plan = OLD.plan AND q.key = OLD.needs AND q.repo = r.repo AND q.lead_branch = r.base)
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;
