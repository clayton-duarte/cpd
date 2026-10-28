-- cpd-migration: breaking after 75
-- The restack of a `waiting` PR after the lock (contract §3.8 "Stacked PRs" step 10, §4.4, §4.5; pro-d133, pro-d137).
-- `prs_locked` gains a third alternative: a `waiting` PR's `base` may change to the Lead Branch of a PR of its Plan and
-- repo. `pr_needs_no_delete` allows deleting a need that is the base PR of a `waiting` PR. No table, column or row changes.

DROP TRIGGER prs_locked;
CREATE TRIGGER prs_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON prs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT (OLD.status IN ('waiting', 'dispatch', 'open')
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.lead_branch IS OLD.lead_branch
          AND NEW.base IS NOT OLD.base
          AND (OLD.status = 'open'
               OR EXISTS (SELECT 1 FROM prs q WHERE q.repo = OLD.repo AND q.lead_branch = OLD.base)
               OR (OLD.status = 'waiting' AND EXISTS (SELECT 1 FROM prs q WHERE q.plan = OLD.plan AND q.repo = OLD.repo AND q.lead_branch = NEW.base))))
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;

DROP TRIGGER pr_needs_no_delete;
CREATE TRIGGER pr_needs_no_delete BEFORE DELETE ON pr_needs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT ((SELECT deleted_at FROM plans WHERE id = OLD.plan) IS NOT NULL)
 AND NOT EXISTS (SELECT 1 FROM prs r, prs q WHERE r.plan = OLD.plan AND r.key = OLD.pr AND r.status = 'waiting' AND q.plan = OLD.plan AND q.key = OLD.needs AND q.repo = r.repo AND q.lead_branch = r.base)
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
