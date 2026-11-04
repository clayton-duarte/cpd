-- cpd-migration: breaking after 108
-- Finish legacy codereview Plans stuck in grounding: grounding > done, legal for codereview (plans_phase trigger).
UPDATE plans SET phase = 'done',
  locked_at = coalesce(locked_at, (SELECT at FROM txns WHERE id = cpd_txn())),
  done_at = coalesce(done_at, (SELECT at FROM txns WHERE id = cpd_txn()))
WHERE context = 'codereview' AND phase = 'grounding' AND deleted_at IS NULL
  AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = plans.id AND j.status IN ('draft', 'queued', 'running', 'failed'))
  AND NOT EXISTS (SELECT 1 FROM inbox_items i WHERE i.session = plans.lead_session AND i.closed_at IS NULL AND i.dismissed_at IS NULL);
