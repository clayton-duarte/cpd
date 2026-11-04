-- cpd-migration: breaking after 106
-- workflow-base-retarget (pro-d379): after the lock a waiting, dispatch or open Workflow's `base` may change to any other
-- value while its plan, key, repo and lead_branch stay; `workflows_locked` drops the conditions about another Workflow's lead branch.
DROP TRIGGER workflows_locked;

CREATE TRIGGER workflows_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON workflows
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT (OLD.status IN ('waiting', 'dispatch', 'open')
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.lead_branch IS OLD.lead_branch
          AND NEW.base IS NOT OLD.base)
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;
