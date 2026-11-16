-- cpd-migration: breaking after 147
-- Permit paired shaping Plans to enter planning and unlocked exploration rounds to settle.
-- This changes only the phase trigger; all rows, CHECKs, and other triggers remain unchanged.
DROP TRIGGER plans_phase;
CREATE TRIGGER plans_phase BEFORE UPDATE OF phase ON plans
WHEN OLD.phase <> NEW.phase AND (OLD.phase || '>' || NEW.phase) NOT IN
  ('grounding>plan', 'plan>dispatch', 'dispatch>deploy', 'deploy>done', 'deploy>dispatch', 'done>dispatch', 'done>deploy',
   'dispatch>plan', 'deploy>plan', 'done>plan', 'shaping>done', 'done>shaping')
  AND NOT (OLD.phase = 'grounding' AND NEW.phase = 'done' AND NEW.context = 'codereview')
  AND NOT (OLD.phase = 'shaping' AND NEW.phase = 'plan' AND OLD.shaping_origin = 1 AND OLD.lead_session IS NOT NULL AND OLD.locked_at IS NULL)
  AND NOT (OLD.phase = 'plan' AND NEW.phase = 'done' AND OLD.shaping_origin = 1 AND OLD.lead_session IS NOT NULL AND OLD.locked_at IS NULL)
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
