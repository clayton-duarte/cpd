-- cpd-migration: breaking after 102
-- workflow-reopen (pro-d362 A1, A3, A6): a Workflow cancelled after its pull request closed comes back when GitHub reopens that PR.
--   * `workflows` is rebuilt (`ALTER TABLE` cannot change a CHECK; the runner applies a migration with foreign keys off and
--     `legacy_alter_table` on, as `20261102T1000-lead-branch-name` relied on) so that a `cancelled` row may keep `pr_number`, `pr_url`,
--     `pr_opened_at` and `pull_request`; it still carries no `lead_worktree` and no `started_at`. `open` and `merged` still require
--     `pr_number` and `pull_request`.
--   * `workflows_status` also allows `cancelled>open`; `plans_phase` also allows `done>deploy` (the `codereview` clause stays).
--   * the Workflow Event kind `workflow-reopened`.
-- Every row and column is copied; the history triggers of `workflows` are the reconcile's.
--
-- Breaking: a rebuild, and an old binary would refuse, at its CHECK, a cancelled Workflow that keeps its pull request.

CREATE TABLE workflows_new (
  plan               TEXT NOT NULL REFERENCES plans(id),
  key                TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 24),
  repo               TEXT NOT NULL CHECK (length(repo) >= 1),
  base               TEXT NOT NULL CHECK (length(base) >= 1),
  status             TEXT NOT NULL CHECK (status IN ('waiting', 'dispatch', 'open', 'merged', 'cancelled')),
  lead_branch        TEXT NOT NULL CHECK (lead_branch IN (plan || '/' || key, 'cpd/' || plan || '/' || key, 'cpd/' || plan)),
  lead_worktree      TEXT,
  pr_number          INTEGER,
  pr_url             TEXT,
  pr_opened_at       TEXT,
  pull_request       INTEGER REFERENCES pull_requests(id),
  all_merged_at      TEXT,
  start_failed_at    TEXT,
  created_txn        INTEGER NOT NULL REFERENCES txns(id),
  created_at         TEXT NOT NULL,
  started_at         TEXT,
  merged_at          TEXT,
  name               TEXT CHECK (name IS NULL OR length(name) BETWEEN 1 AND 100),
  title              TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 72),
  shape              TEXT NOT NULL DEFAULT 'free' CHECK (shape IN ('tdd', 'docs', 'config', 'free')),
  head_ref           TEXT CHECK (head_ref IS NULL OR length(head_ref) BETWEEN 1 AND 255),
  description        TEXT CHECK (description IS NULL OR length(description) BETWEEN 1 AND 65536),
  jira               TEXT,
  PRIMARY KEY (plan, key),
  UNIQUE (plan, lead_branch),
  CHECK ((status IN ('waiting', 'cancelled')) = (lead_worktree IS NULL)),
  CHECK ((status IN ('waiting', 'cancelled')) = (started_at IS NULL)),
  CHECK (status = 'cancelled' OR (status IN ('open', 'merged')) = (pr_number IS NOT NULL)),   -- pro-d362: a cancelled row may keep its PR (A1); open and merged need one, waiting and dispatch have none
  CHECK ((pr_number IS NULL) = (pr_url IS NULL) AND (pr_number IS NULL) = (pr_opened_at IS NULL)),
  CHECK (status = 'cancelled' OR (status IN ('open', 'merged')) = (pull_request IS NOT NULL)),   -- pro-d362, as pr_number
  CHECK ((status = 'merged') = (merged_at IS NOT NULL)),
  CHECK (start_failed_at IS NULL OR status = 'waiting')
) STRICT;
INSERT INTO workflows_new (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                           pull_request, all_merged_at, start_failed_at, created_txn, created_at, started_at, merged_at,
                           name, title, shape, head_ref, description, jira)
  SELECT plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
         pull_request, all_merged_at, start_failed_at, created_txn, created_at, started_at, merged_at,
         name, title, shape, head_ref, description, jira
  FROM workflows;
DROP TABLE workflows;
ALTER TABLE workflows_new RENAME TO workflows;

CREATE UNIQUE INDEX workflows_head_ref_live ON workflows (repo, head_ref)
  WHERE head_ref IS NOT NULL AND status IN ('waiting', 'dispatch', 'open');

CREATE TRIGGER workflows_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON workflows
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT (OLD.status IN ('waiting', 'dispatch', 'open')
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.lead_branch IS OLD.lead_branch
          AND NEW.base IS NOT OLD.base
          AND (OLD.status = 'open'
               OR EXISTS (SELECT 1 FROM workflows q WHERE q.repo = OLD.repo AND q.lead_branch = OLD.base)
               OR (OLD.status = 'waiting' AND EXISTS (SELECT 1 FROM workflows q WHERE q.plan = OLD.plan AND q.repo = OLD.repo AND q.lead_branch = NEW.base))))
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

CREATE TRIGGER workflows_status BEFORE UPDATE OF status ON workflows
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('waiting>dispatch', 'dispatch>open', 'open>merged', 'waiting>cancelled', 'dispatch>cancelled', 'open>cancelled', 'cancelled>open')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER workflows_shape_fixed BEFORE UPDATE OF shape ON workflows
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

CREATE TRIGGER workflows_head_ref_fixed BEFORE UPDATE OF head_ref ON workflows
BEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;

DROP TRIGGER plans_phase;

CREATE TRIGGER plans_phase BEFORE UPDATE OF phase ON plans
WHEN OLD.phase <> NEW.phase AND (OLD.phase || '>' || NEW.phase) NOT IN
  ('grounding>plan', 'plan>dispatch', 'dispatch>deploy', 'deploy>done', 'deploy>dispatch', 'done>dispatch', 'done>deploy')
  AND NOT (OLD.phase = 'grounding' AND NEW.phase = 'done' AND NEW.context = 'codereview')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

INSERT INTO event_kinds (name) VALUES ('workflow-reopened');
