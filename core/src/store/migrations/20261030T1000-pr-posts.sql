-- cpd-migration: additive
-- The post of a PR for review in Slack (pro-d64, pro-d65; Plan pr-post; contract §4.4, §4.5 `pr-posts`): one row per PR, written once by
-- `cpd pr post`; the only later write is `reacted_at`. The history triggers are generated.
CREATE TABLE pr_posts (
  plan        TEXT NOT NULL REFERENCES plans(id),
  pr          TEXT NOT NULL,
  channel     TEXT NOT NULL CHECK (length(channel) BETWEEN 1 AND 20),
  ts          TEXT NOT NULL CHECK (length(ts) >= 1),
  link        TEXT NOT NULL CHECK (length(link) >= 1),
  posted_at   TEXT NOT NULL,
  reaction    TEXT CHECK (reaction IS NULL OR length(reaction) BETWEEN 1 AND 100),
  reacted_at  TEXT,
  PRIMARY KEY (plan, pr),
  FOREIGN KEY (plan, pr) REFERENCES prs(plan, key),
  CHECK (reaction IS NOT NULL OR reacted_at IS NULL)
) STRICT;
INSERT INTO history_tables (name, no_delete) VALUES ('pr_posts', 1);
