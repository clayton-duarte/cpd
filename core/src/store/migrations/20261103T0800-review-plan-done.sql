-- cpd-migration: breaking after 99
-- review-plan-done (FR10): `review submit` finishes a `codereview` Plan from `grounding` to `done`. `plans_phase` is recreated as `0008-phase-deploy`
-- defines it, with one more allowed transition, `grounding>done`, for Context `codereview` only. No row and no table changes.
DROP TRIGGER plans_phase;

CREATE TRIGGER plans_phase BEFORE UPDATE OF phase ON plans
WHEN OLD.phase <> NEW.phase AND (OLD.phase || '>' || NEW.phase) NOT IN
  ('grounding>plan', 'plan>dispatch', 'dispatch>deploy', 'deploy>done', 'deploy>dispatch', 'done>dispatch')
  AND NOT (OLD.phase = 'grounding' AND NEW.phase = 'done' AND NEW.context = 'codereview')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
