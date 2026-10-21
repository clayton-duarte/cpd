-- cpd-migration: breaking after 36
-- The tombstone of a deleted Plan or Context (contract §2.2 "Deleted", §4.4 "The delete exception", §4.5; hs-d527).
-- `plan delete` and `context delete` set `deleted_at` first and then delete the rows of the Plan or Context through
-- the one exception each delete trigger gains. The rows of `plans` and `contexts` are never deleted. Rewrites no row:
-- every existing `deleted_at` is null.
--
-- Breaking (A1): it replaces triggers that refuse a delete with ones that allow one (a trigger change that only
-- relaxes), creates triggers on tables that exist, and changes the generator: the reconcile after it replaces every
-- `T_no_delete` (they now hold the exception) and creates every `T_hist_del`.

ALTER TABLE contexts ADD COLUMN deleted_at TEXT;
ALTER TABLE plans ADD COLUMN deleted_at TEXT;

INSERT INTO event_kinds (name) VALUES ('plan-deleted'), ('context-deleted');

CREATE TRIGGER plans_deleted_once BEFORE UPDATE OF deleted_at ON plans
WHEN OLD.deleted_at IS NOT NULL AND NEW.deleted_at IS NOT OLD.deleted_at
BEGIN SELECT RAISE(ABORT, 'append_only: plans.deleted_at'); END;
CREATE TRIGGER contexts_deleted_once BEFORE UPDATE OF deleted_at ON contexts
WHEN OLD.deleted_at IS NOT NULL AND NEW.deleted_at IS NOT OLD.deleted_at
BEGIN SELECT RAISE(ABORT, 'append_only: contexts.deleted_at'); END;

DROP TRIGGER events_no_delete;
CREATE TRIGGER events_no_delete BEFORE DELETE ON events
WHEN NOT ((SELECT deleted_at FROM plans WHERE id = OLD.plan) IS NOT NULL
          OR (OLD.plan IS NULL AND (SELECT deleted_at FROM contexts WHERE id = OLD.context) IS NOT NULL))
BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;

DROP TRIGGER pr_needs_no_delete;
CREATE TRIGGER pr_needs_no_delete BEFORE DELETE ON pr_needs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT ((SELECT deleted_at FROM plans WHERE id = OLD.plan) IS NOT NULL)
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;

DROP TRIGGER dependencies_no_delete;
CREATE TRIGGER dependencies_no_delete BEFORE DELETE ON dependencies
WHEN (SELECT status FROM jobs WHERE plan = OLD.plan AND key = OLD.job) <> 'draft'
 AND NOT ((SELECT deleted_at FROM plans WHERE id = OLD.plan) IS NOT NULL)
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;
