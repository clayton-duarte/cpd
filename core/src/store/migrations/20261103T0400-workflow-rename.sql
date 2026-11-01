-- cpd-migration: breaking after 86
-- The object is a Workflow (Plan glossary-rename, Context hs-d805; design docs/research/glossary-rename.md §3.4
-- and §3.11): the object tables `prs` and `pr_needs` become `workflows` and `workflow_needs`, their object columns
-- rename, the hand-written triggers are recreated under the new names, `history_tables` follows, the stored
-- `jobs.cancel_reason` value is rewritten (R14a) and `SCHEMA_HASH` is re-pinned. A rename, not a rebuild: every row
-- survives and no `history` row is written (the append-only history keeps its `tbl` values). Breaking: it renames
-- tables, columns, a unique index and the hand-written triggers, and an older binary would refuse the new names.
--
-- The runner applies a migration with `foreign_keys` off and `legacy_alter_table` on; under `legacy_alter_table`
-- on, `ALTER TABLE ... RENAME` does not rewrite the foreign-key clauses other tables hold to the renamed table, so
-- the rename below runs with it off for the duration of this migration (the runner restores it after).

PRAGMA legacy_alter_table = OFF;

-- 1. Drop the hand-written triggers, the index and the generated history triggers that name the object, so nothing
-- is left pointing at a table that no longer exists (the reconcile regenerates the history triggers, K4).
-- `IF EXISTS`: on a fresh store the rebuilds of `prs`, `pr_needs` and `jobs` dropped the generated history
-- triggers and the reconcile of this transaction has not recreated them yet, while an existing store reconciled
-- them at its last open; both paths must end with the same schema.
DROP TRIGGER IF EXISTS prs_locked;
DROP TRIGGER IF EXISTS prs_status;
DROP TRIGGER IF EXISTS prs_shape_fixed;
DROP TRIGGER IF EXISTS prs_head_ref_fixed;
DROP TRIGGER IF EXISTS pr_needs_locked;
DROP TRIGGER IF EXISTS pr_needs_no_delete;
DROP TRIGGER IF EXISTS pr_needs_hist_del;
DROP TRIGGER IF EXISTS jobs_pr_dependencies;
DROP TRIGGER IF EXISTS dependencies_same_pr;
DROP INDEX IF EXISTS prs_head_ref_live;
DROP TRIGGER IF EXISTS prs_hist_ins;
DROP TRIGGER IF EXISTS prs_hist_upd;
DROP TRIGGER IF EXISTS prs_hist_del;
DROP TRIGGER IF EXISTS prs_no_delete;
DROP TRIGGER IF EXISTS pr_needs_hist_ins;
DROP TRIGGER IF EXISTS pr_needs_hist_upd;

-- 2. Rename the tables and their object columns. A rename keeps every row and writes no `history` row.
ALTER TABLE prs RENAME TO workflows;
ALTER TABLE pr_needs RENAME TO workflow_needs;
ALTER TABLE jobs RENAME COLUMN pr TO workflow;
ALTER TABLE workflow_needs RENAME COLUMN pr TO workflow;
ALTER TABLE pr_posts RENAME COLUMN pr TO workflow;
ALTER TABLE slack_reactions RENAME COLUMN pr TO workflow;
ALTER TABLE incident RENAME COLUMN until_pr TO until_workflow;

-- 3. Rename the rows of `history_tables`, so the reconcile regenerates the generated triggers under the new names.
UPDATE history_tables SET name = 'workflows' WHERE name = 'prs';
UPDATE history_tables SET name = 'workflow_needs' WHERE name = 'pr_needs';

-- 4. Rewrite the stored `cancel_reason` (R14a). The `job-cancelled` Events' `reason` is append-only history and
-- keeps the old value.
UPDATE jobs SET cancel_reason = 'workflow cancelled' WHERE cancel_reason = 'pr cancelled';

-- 5. Recreate the index and the hand-written triggers under the new names and text.
CREATE UNIQUE INDEX workflows_head_ref_live ON workflows (repo, head_ref)
  WHERE head_ref IS NOT NULL AND status IN ('waiting', 'dispatch', 'open');

CREATE TRIGGER workflows_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON workflows
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT (OLD.status IN ('waiting', 'dispatch', 'open')
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.lead_branch IS OLD.lead_branch
          AND NEW.base IS NOT OLD.base
          AND (OLD.status = 'open'
               OR EXISTS (SELECT 1 FROM workflows q WHERE q.repo = OLD.repo AND q.lead_branch = OLD.base)
               OR (OLD.status = 'waiting' AND EXISTS (SELECT 1 FROM workflows q WHERE q.plan = OLD.plan AND q.repo = OLD.repo AND q.lead_branch = NEW.base))))
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

CREATE TRIGGER workflows_status BEFORE UPDATE OF status ON workflows
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('waiting>dispatch', 'dispatch>open', 'open>merged', 'waiting>cancelled', 'dispatch>cancelled', 'open>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER workflows_shape_fixed BEFORE UPDATE OF shape ON workflows
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

CREATE TRIGGER workflows_head_ref_fixed BEFORE UPDATE OF head_ref ON workflows
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

CREATE TRIGGER workflow_needs_locked BEFORE INSERT ON workflow_needs
WHEN (SELECT phase FROM plans WHERE id = NEW.plan) NOT IN ('grounding', 'plan')
 AND (SELECT created_txn FROM workflows WHERE plan = NEW.plan AND key = NEW.workflow) <> cpd_txn()
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

CREATE TRIGGER workflow_needs_no_delete BEFORE DELETE ON workflow_needs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT ((SELECT deleted_at FROM plans WHERE id = OLD.plan) IS NOT NULL)
 AND NOT EXISTS (SELECT 1 FROM workflows r, workflows q WHERE r.plan = OLD.plan AND r.key = OLD.workflow AND r.status = 'waiting' AND q.plan = OLD.plan AND q.key = OLD.needs AND q.repo = r.repo AND q.lead_branch = r.base)
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

CREATE TRIGGER workflow_needs_hist_del AFTER DELETE ON workflow_needs BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'workflow_needs',
    json_array(OLD.plan, OLD.workflow, OLD.needs), 'delete',
    json_object('plan', OLD.plan, 'workflow', OLD.workflow, 'needs', OLD.needs), NULL);
END;

CREATE TRIGGER dependencies_same_workflow BEFORE INSERT ON dependencies
WHEN (SELECT workflow FROM jobs WHERE plan = NEW.plan AND key = NEW.job)
  IS NOT (SELECT workflow FROM jobs WHERE plan = NEW.plan AND key = NEW.needs)
BEGIN SELECT RAISE(ABORT, 'needs_other_workflow'); END;

CREATE TRIGGER jobs_workflow_dependencies BEFORE UPDATE OF workflow ON jobs
WHEN OLD.workflow <> NEW.workflow AND EXISTS (SELECT 1 FROM dependencies WHERE plan = NEW.plan AND (job = NEW.key OR needs = NEW.key))
BEGIN SELECT RAISE(ABORT, 'needs_other_workflow'); END;
