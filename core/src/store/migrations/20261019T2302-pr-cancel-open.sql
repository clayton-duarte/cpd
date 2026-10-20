-- cpd-migration: breaking after 20
-- An `open` PR whose pull request was closed on GitHub without a merge may be cancelled (contract §3.8, commands.md
-- §9.6, hs-d329): `prs_status` gains `open>cancelled`. Which `open` PRs may is `pr cancel`'s rule (the stored
-- `pull_requests` row is `closed`), not the trigger's: it sees no other table. A trigger change that only relaxes is
-- still breaking (§4.5 A1).

DROP TRIGGER prs_status;
CREATE TRIGGER prs_status BEFORE UPDATE OF status ON prs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('waiting>dispatch', 'dispatch>open', 'open>merged', 'waiting>cancelled', 'dispatch>cancelled', 'open>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
