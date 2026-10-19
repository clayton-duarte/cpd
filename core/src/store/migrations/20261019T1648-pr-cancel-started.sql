-- cpd-migration: breaking after 18
-- A started PR (`dispatch`) may be cancelled after the lock (contract §3.8, commands.md §9.6): `prs_status` gains
-- `dispatch>cancelled`. A trigger change that only relaxes is still breaking (§4.5 A1).

DROP TRIGGER prs_status;
CREATE TRIGGER prs_status BEFORE UPDATE OF status ON prs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('waiting>dispatch', 'dispatch>open', 'open>merged', 'waiting>cancelled', 'dispatch>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
