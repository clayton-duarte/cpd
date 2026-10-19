-- Migration 6 (contract §4.5, the PR poller, hs-d96, D5, D6): the table
-- `pull_requests`, and `prs` rebuilt without the poll columns and with
-- `pull_request`. The table and trigger texts are §4.4's as written.
-- schema.rs runs it with foreign keys off and legacy_alter_table on (step 1),
-- in two parts split at the line `-- @history`: between them it generates the
-- history triggers of `pull_requests`, so the back-fill writes history rows
-- (step 3); after the second part it generates those of `prs` (step 5).
-- Frozen: the schema hash of version 6 covers this text. Change the schema
-- only with a new migration.

-- ---------- step 2: the table of pull requests ----------
CREATE TABLE pull_requests (
  id              INTEGER PRIMARY KEY,
  repo            TEXT NOT NULL CHECK (repo LIKE '_%/_%'),      -- GitHub's owner/name
  number          INTEGER NOT NULL CHECK (number >= 1),
  url             TEXT NOT NULL,
  title           TEXT NOT NULL CHECK (length(title) >= 1),
  draft           INTEGER NOT NULL CHECK (draft IN (0, 1)),
  state           TEXT NOT NULL CHECK (state IN ('open', 'closed', 'merged')),
  review_decision TEXT NOT NULL CHECK (review_decision IN ('none', 'approved', 'changes_requested')),
  checks          TEXT NOT NULL CHECK (checks IN ('none', 'passed', 'pending', 'failed')),
  created_at      TEXT NOT NULL,                               -- GitHub's
  merged_at       TEXT,
  changed_at      TEXT NOT NULL,                               -- cpd's time of the last change
  UNIQUE (repo, number),
  CHECK ((state = 'merged') = (merged_at IS NOT NULL))
) STRICT;

CREATE TRIGGER pull_requests_hist_del AFTER DELETE ON pull_requests BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'pull_requests',
    json_array(OLD.id), 'delete',
    json_object('id', OLD.id, 'repo', OLD.repo, 'number', OLD.number, 'url', OLD.url, 'title', OLD.title,
                'draft', OLD.draft, 'state', OLD.state, 'review_decision', OLD.review_decision,
                'checks', OLD.checks, 'created_at', OLD.created_at, 'merged_at', OLD.merged_at,
                'changed_at', OLD.changed_at), NULL);
END;

CREATE TRIGGER pull_requests_owned BEFORE DELETE ON pull_requests
WHEN EXISTS (SELECT 1 FROM prs WHERE pull_request = OLD.id)
BEGIN SELECT RAISE(ABORT, 'append_only: pull_requests owned by a PR'); END;

-- @history

-- ---------- step 3: back-fill, one row for each PR with a pull request ----------
-- `pr_url` is https://github.com/<owner>/<name>/pull/<n>: 19 characters of host, then the repo.
INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
  SELECT substr(r.pr_url, 20, instr(substr(r.pr_url, 20), '/pull/') - 1), r.pr_number, r.pr_url,
         coalesce(r.pr_title, p.title), 0,
         CASE r.status WHEN 'merged' THEN 'merged' ELSE 'open' END,
         coalesce(r.pr_review_decision, 'none'), coalesce(r.pr_checks, 'none'),
         r.pr_opened_at, CASE r.status WHEN 'merged' THEN r.merged_at END, r.pr_opened_at
  FROM prs r JOIN plans p ON p.id = r.plan
  WHERE r.pr_number IS NOT NULL
  ORDER BY r.plan, r.key;

-- ---------- step 4: rebuild prs ----------
CREATE TABLE prs_new (
  plan               TEXT NOT NULL REFERENCES plans(id),
  key                TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 24),
  repo               TEXT NOT NULL CHECK (length(repo) >= 1),
  base               TEXT NOT NULL CHECK (length(base) >= 1),
  status             TEXT NOT NULL CHECK (status IN ('waiting', 'dispatch', 'open', 'merged', 'cancelled')),
  lead_branch        TEXT NOT NULL CHECK (lead_branch IN ('cpd/' || plan || '/' || key, 'cpd/' || plan)),  -- the second form only for PRs made by migration 4
  lead_worktree      TEXT,
  pr_number          INTEGER,
  pr_url             TEXT,
  pr_opened_at       TEXT,
  pull_request       INTEGER REFERENCES pull_requests(id),   -- migration 6 (hs-d96): set by pr open
  all_merged_at      TEXT,
  start_failed_at    TEXT,
  created_txn        INTEGER NOT NULL REFERENCES txns(id),
  created_at         TEXT NOT NULL,
  started_at         TEXT,
  merged_at          TEXT,
  PRIMARY KEY (plan, key),
  UNIQUE (plan, lead_branch),
  CHECK ((status IN ('waiting', 'cancelled')) = (lead_worktree IS NULL)),
  CHECK ((status IN ('waiting', 'cancelled')) = (started_at IS NULL)),
  CHECK ((status IN ('open', 'merged')) = (pr_number IS NOT NULL)),
  CHECK ((pr_number IS NULL) = (pr_url IS NULL) AND (pr_number IS NULL) = (pr_opened_at IS NULL)),
  CHECK ((status IN ('open', 'merged')) = (pull_request IS NOT NULL)),
  CHECK ((status = 'merged') = (merged_at IS NOT NULL)),
  CHECK (start_failed_at IS NULL OR status = 'waiting')
) STRICT;
INSERT INTO prs_new (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                     pull_request, all_merged_at, start_failed_at, created_txn, created_at, started_at, merged_at)
  SELECT plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
         (SELECT id FROM pull_requests WHERE pull_requests.url = prs.pr_url),
         all_merged_at, start_failed_at, created_txn, created_at, started_at, merged_at
  FROM prs;
DROP TABLE prs;
ALTER TABLE prs_new RENAME TO prs;

-- ---------- step 5: the triggers of prs ----------
-- R34: a PR's identity, repo and base are Locked once its Plan is locked; needs only at the PR's own insert (pr add in dispatch)
-- Migration 5's text (hs-d118): one exception, the legacy Lead Branch of a merged PR moves to `cpd/<plan>/<key>` (pr add).
CREATE TRIGGER prs_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON prs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT (OLD.status = 'merged'
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.base IS OLD.base
          AND OLD.lead_branch = 'cpd/' || OLD.plan
          AND NEW.lead_branch = 'cpd/' || OLD.plan || '/' || OLD.key)
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;

CREATE TRIGGER prs_status BEFORE UPDATE OF status ON prs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('waiting>dispatch', 'dispatch>open', 'open>merged', 'waiting>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
