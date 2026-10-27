-- cpd-migration: breaking after 58
-- pro-d79: the Spec of a queued Job with no Attempt may change (ADR-0014); `jobs_locked` is replaced, no table changes.
DROP TRIGGER jobs_locked;
CREATE TRIGGER jobs_locked BEFORE UPDATE OF title, tier, goal, context, touches, acceptance, out_of_scope, plan, key, pr, review, kind, tests, test_files ON jobs
WHEN OLD.status <> 'draft'
 AND NOT (OLD.status = 'queued'
          AND OLD.kind NOT IN ('open', 'update')
          AND NOT EXISTS (SELECT 1 FROM attempts WHERE plan = OLD.plan AND job = OLD.key)
          AND NEW.title IS OLD.title AND NEW.tier IS OLD.tier AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key
          AND NEW.pr IS OLD.pr AND NEW.review IS OLD.review AND NEW.kind IS OLD.kind)
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;
