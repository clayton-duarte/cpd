-- cpd-migration: breaking after 51
-- Cancel fails its dependents (hs-d647; contract §3.2 "Cancel", §4.4): a `queued` Job whose need, directly or
-- transitively, was cancelled becomes `failed` in the transaction that cancels the need, so `jobs_status` must
-- allow `queued>failed`. Breaking: an old binary that lacks this migration does not know the transition, while
-- one that knows it could write rows an old binary refuses. The trigger's body is 0014-register.sql's, with
-- `queued>failed` added. Nothing else changes: no table, column, check or row.
DROP TRIGGER jobs_status;
CREATE TRIGGER jobs_status BEFORE UPDATE OF status ON jobs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('draft>queued', 'queued>running', 'running>merged',
   'running>queued', 'running>failed', 'failed>queued', 'queued>failed',
   'draft>cancelled', 'queued>cancelled', 'failed>cancelled', 'running>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
