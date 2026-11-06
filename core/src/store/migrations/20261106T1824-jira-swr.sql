-- cpd-migration: breaking after 116
-- jira-swr (docs/architecture/swr-jira.md §12; pro-d443): the Jira transition queue moves onto the SWR mutation outbox.
--   * every open row of `jira_transitions` (PENDING, or PARKED as refused) is copied into `swr_mutations` as a `jira`
--     `transition`; a Workflow with no ticket gets a PENDING row with no `target_key`, which the outbox never claims until a
--     ticket is set. Rows that settled are not copied: a finished move has no work left. `_rank` in `context_json` is the
--     rank `Coalesce::Forward` reads.
--   * a pending row ranked below another pending row of its key is superseded, once.
--   * `jira_transitions` is dropped.
-- Breaking: a dropped table, which an old binary reads.

INSERT INTO swr_mutations (system, target_key, op_kind, vars_json, patch_json, status, submitted_at, context_json, error)
SELECT 'jira',
       'jira:issue:' || p.jira,
       'transition',
       json_object('ticket', p.jira, 'target', t.target,
                   'rank', CASE t.target WHEN 'in_progress' THEN 1 WHEN 'in_review' THEN 2 ELSE 3 END),
       NULL,
       CASE WHEN t.refused_at IS NULL THEN 'pending' ELSE 'failed' END,
       t.queued_at,
       CASE WHEN t.refused_at IS NULL
            THEN json_object('plan', t.plan, 'workflow', t.pr,
                             '_rank', CASE t.target WHEN 'in_progress' THEN 1 WHEN 'in_review' THEN 2 ELSE 3 END)
            ELSE json_object('plan', t.plan, 'workflow', t.pr, 'refused_with', t.refused_with,
                             '_rank', CASE t.target WHEN 'in_progress' THEN 1 WHEN 'in_review' THEN 2 ELSE 3 END) END,
       t.refusal
FROM jira_transitions t JOIN workflows p ON p.plan = t.plan AND p.key = t.pr
WHERE t.settled_at IS NULL AND p.jira IS NOT NULL
ORDER BY t.id;

INSERT INTO swr_mutations (system, target_key, op_kind, vars_json, status, submitted_at, context_json)
SELECT 'jira', NULL, 'transition',
       json_object('target', t.target,
                   'rank', CASE t.target WHEN 'in_progress' THEN 1 WHEN 'in_review' THEN 2 ELSE 3 END),
       'pending', t.queued_at,
       json_object('plan', t.plan, 'workflow', t.pr,
                   '_rank', CASE t.target WHEN 'in_progress' THEN 1 WHEN 'in_review' THEN 2 ELSE 3 END)
FROM jira_transitions t JOIN workflows p ON p.plan = t.plan AND p.key = t.pr
WHERE t.settled_at IS NULL AND t.refused_at IS NULL AND p.jira IS NULL
ORDER BY t.id;

UPDATE swr_mutations SET status = 'superseded'
WHERE system = 'jira' AND status = 'pending' AND target_key IS NOT NULL
  AND json_extract(vars_json, '$.rank') < (SELECT MAX(json_extract(b.vars_json, '$.rank')) FROM swr_mutations b
        WHERE b.system = 'jira' AND b.status = 'pending' AND b.target_key = swr_mutations.target_key);

DROP TABLE jira_transitions;
