-- cpd-migration: breaking after 116
-- swr-slack (docs/architecture/swr-slack.md §2): one row per (message, pull request); the helper's author and primary flag.
-- The table holds no row a user wrote (the first fetch fills it), so it is rebuilt empty-compatible: open rows are copied.
CREATE TABLE swr_slack_posts_new (
  channel        TEXT NOT NULL,
  ts             TEXT NOT NULL,
  repo           TEXT NOT NULL,
  number         INTEGER NOT NULL,
  primary_pr     INTEGER NOT NULL CHECK (primary_pr IN (0, 1)),
  author_id      TEXT,
  author         TEXT,
  author_is_me   INTEGER NOT NULL CHECK (author_is_me IN (0, 1)),
  reply_count    INTEGER,
  latest_reply   TEXT,
  reactions_json TEXT NOT NULL,
  claim_json     TEXT,
  eyes_me        INTEGER CHECK (eyes_me IS NULL OR eyes_me IN (0, 1)),
  thread_tags_me INTEGER CHECK (thread_tags_me IS NULL OR thread_tags_me IN (0, 1)),
  fetched_at     TEXT NOT NULL,
  inactive_at    TEXT,
  PRIMARY KEY (channel, ts, repo, number)
) STRICT;
INSERT INTO swr_slack_posts_new (channel, ts, repo, number, primary_pr, author_is_me, reply_count, latest_reply, reactions_json, claim_json, eyes_me, thread_tags_me, fetched_at, inactive_at)
  SELECT channel, ts, repo, number, 1, COALESCE(author_is_me, 0), reply_count, latest_reply, COALESCE(reactions_json, '{}'), claim, eyes_me, thread_tags_me, COALESCE(fetched_at, ''), inactive_at FROM swr_slack_posts;
DROP TABLE swr_slack_posts;
ALTER TABLE swr_slack_posts_new RENAME TO swr_slack_posts;
-- The Slack reaction queue moves to the outbox (swr-slack.md §9): a row still open becomes a `pending` `reaction` mutation, a parked one
-- (refused) a `failed` one with its refusal as the error; a reacted row is dropped. The key order of `vars_json` is the canonical one
-- (sorted, no spaces) that `Dedupe` compares. The table stays, empty: nothing writes it any more.
INSERT INTO swr_mutations (system, target_key, op_kind, vars_json, status, submitted_at, completed_at, error)
  SELECT 'slack', 'slack:channel:' || channel, 'reaction',
         CASE WHEN plan IS NULL THEN json_object('channel', channel, 'emoji', emoji, 'ts', ts)
              ELSE json_object('channel', channel, 'emoji', emoji, 'plan', plan, 'ts', ts, 'workflow', workflow) END,
         IIF(refused_at IS NULL, 'pending', 'failed'), queued_at, refused_at, refusal
    FROM slack_reactions WHERE reacted_at IS NULL ORDER BY id;
DELETE FROM slack_reactions;
