-- Migration 5 (hs-d118, hs-d119): a Plan takes follow-up work at any time.
-- No table changes; two triggers are dropped and created again.

-- `pr>dispatch` and `done>dispatch` are legal: `pr add` reopens the Plan. The
-- CHECK ((phase = 'done') = (done_at IS NOT NULL)) clears `done_at` with it.
DROP TRIGGER plans_phase;
CREATE TRIGGER plans_phase BEFORE UPDATE OF phase ON plans
WHEN OLD.phase <> NEW.phase AND (OLD.phase || '>' || NEW.phase) NOT IN
  ('grounding>plan', 'plan>dispatch', 'dispatch>pr', 'pr>done', 'pr>dispatch', 'done>dispatch')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

-- One exception: the legacy Lead Branch of a merged PR moves to `cpd/<plan>/<key>` (pr add).
DROP TRIGGER prs_locked;
CREATE TRIGGER prs_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON prs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT (OLD.status = 'merged'
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.base IS OLD.base
          AND OLD.lead_branch = 'cpd/' || OLD.plan
          AND NEW.lead_branch = 'cpd/' || OLD.plan || '/' || OLD.key)
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
