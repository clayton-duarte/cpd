-- cpd-migration: additive
-- swr (docs/architecture/swr.md §3; contract §4.4): the stale-while-revalidate cache, the endpoint health, the mutation outbox
-- and the datasource tables. Data tables: no history triggers, no Event kind. `pull_requests` gains two nullable columns.
ALTER TABLE pull_requests ADD COLUMN head_oid TEXT;
ALTER TABLE pull_requests ADD COLUMN gh_updated_at TEXT;

CREATE TABLE swr_queries (
  key         TEXT PRIMARY KEY,                                  -- '<system>:<kind>:<id>', e.g. github:pr:owner/repo#12
  system      TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'success', 'error')),
  data_hash   TEXT,
  fetched_at  TEXT,                                              -- the last Changed commit
  invalidated INTEGER NOT NULL DEFAULT 0 CHECK (invalidated IN (0, 1)),
  stale_after TEXT,                                              -- NULL: never stale (Infinity)
  last_error  TEXT,
  error_count INTEGER NOT NULL DEFAULT 0 CHECK (error_count >= 0),
  inactive_at TEXT                                               -- NULL while an observer needs the key
) STRICT;

CREATE TABLE swr_endpoints (
  endpoint       TEXT PRIMARY KEY,                               -- github, slack, jira, ollama.lab, bifrost
  status         TEXT CHECK (status IN ('pending', 'success', 'error')),
  fetch_status   TEXT NOT NULL DEFAULT 'idle' CHECK (fetch_status IN ('idle', 'fetching', 'paused')),
  paused_until   TEXT,
  failing_since  TEXT,
  reported_at    TEXT,
  last_error     TEXT,
  error_count    INTEGER NOT NULL DEFAULT 0 CHECK (error_count >= 0),
  last_ok_at     TEXT,
  rate_remaining INTEGER,
  rate_reset_at  TEXT
) STRICT;

CREATE TABLE swr_mutations (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  system          TEXT,
  target_key      TEXT,                                          -- the query key the write changes
  op_kind         TEXT,
  vars_json       TEXT NOT NULL,
  patch_json      TEXT,                                          -- the overlay patch; NULL when none
  status          TEXT NOT NULL CHECK (status IN ('pending', 'in_flight', 'revalidating', 'done', 'failed', 'superseded')),
  submitted_at    TEXT,
  started_at      TEXT,
  completed_at    TEXT,
  next_attempt_at TEXT,                                          -- NULL: at once
  attempt_count   INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  error           TEXT,
  context_json    TEXT                                           -- plan, Workflow and pull request
) STRICT;
CREATE INDEX swr_mutations_active ON swr_mutations (system, target_key, op_kind, status);

CREATE TABLE swr_viewer (
  id         INTEGER PRIMARY KEY CHECK (id = 1),
  login      TEXT,
  fetched_at TEXT
) STRICT;

CREATE TABLE swr_repos (
  repo                   TEXT PRIMARY KEY,                       -- owner/name
  default_branch         TEXT,
  is_archived            INTEGER CHECK (is_archived IN (0, 1)),
  auto_merge_allowed     INTEGER CHECK (auto_merge_allowed IN (0, 1)),
  squash_merge_allowed   INTEGER CHECK (squash_merge_allowed IN (0, 1)),
  merge_commit_allowed   INTEGER CHECK (merge_commit_allowed IN (0, 1)),
  rebase_merge_allowed   INTEGER CHECK (rebase_merge_allowed IN (0, 1)),
  delete_branch_on_merge INTEGER CHECK (delete_branch_on_merge IN (0, 1)),
  fetched_at             TEXT,
  inactive_at            TEXT
) STRICT;

CREATE TABLE swr_pr_detail (                                     -- the heavy part of a pull request; the header stays in pull_requests
  repo        TEXT NOT NULL,
  number      INTEGER NOT NULL,
  head_oid    TEXT,
  updated_at  TEXT,
  hydrated_at TEXT,
  detail_json TEXT NOT NULL,                                     -- latestReviews, reviewThreads, comments, check runs
  detail_hash TEXT NOT NULL,
  PRIMARY KEY (repo, number)
) STRICT;

CREATE TABLE swr_deploys (
  repo         TEXT NOT NULL,
  run_id       INTEGER NOT NULL,
  updated_at   TEXT,
  run_json     TEXT,
  pending_json TEXT,
  inactive_at  TEXT,
  PRIMARY KEY (repo, run_id)
) STRICT;

CREATE TABLE gh_issues (                                         -- created now, unused until a later Plan polls issues
  repo       TEXT NOT NULL,
  number     INTEGER NOT NULL,
  state      TEXT,
  title      TEXT,
  updated_at TEXT,
  data_json  TEXT,
  fetched_at TEXT,
  inactive_at TEXT,
  PRIMARY KEY (repo, number)
) STRICT;

CREATE TABLE swr_slack_posts (
  channel        TEXT NOT NULL,
  ts             TEXT NOT NULL,
  repo           TEXT,
  number         INTEGER,
  author_is_me   INTEGER,
  reply_count    INTEGER,
  latest_reply   TEXT,
  reactions_json TEXT,
  eyes_me        INTEGER,
  claim          TEXT,
  thread_tags_me INTEGER,
  fetched_at     TEXT,
  inactive_at    TEXT,
  PRIMARY KEY (channel, ts)
) STRICT;

CREATE TABLE swr_jira_issues (
  ticket          TEXT PRIMARY KEY,
  cloud_id        TEXT,
  summary         TEXT,
  status_name     TEXT,
  status_category TEXT,
  estimates_json  TEXT,
  synced_at       TEXT,
  inactive_at     TEXT
) STRICT;

CREATE TABLE swr_atlassian (
  id         INTEGER PRIMARY KEY CHECK (id = 1),
  cloud_id   TEXT,
  fetched_at TEXT
) STRICT;

CREATE TABLE swr_models (
  provider     TEXT PRIMARY KEY,
  catalog_json TEXT,
  changes_json TEXT,
  polled_at    TEXT,
  last_error   TEXT,
  inactive_at  TEXT
) STRICT;

CREATE TABLE swr_badges (
  session_id  TEXT PRIMARY KEY,
  entry_id    TEXT NOT NULL,
  label       TEXT,
  score       REAL,
  text        TEXT,
  tone        TEXT,
  others_json TEXT,
  synced_at   TEXT,
  inactive_at TEXT
) STRICT;
