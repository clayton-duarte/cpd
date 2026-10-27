-- cpd-migration: additive
-- The Slack reaction queue (pro-d107, pro-d110; contract §4.4, §4.5 `slack-reactions`): one row per emoji on a Slack post,
-- oldest first, drained by the engine; `reacted_at` is NULL until the helper succeeded. A data row: a row is never deleted.
CREATE TABLE slack_reactions (
  id          INTEGER PRIMARY KEY,
  channel     TEXT NOT NULL CHECK (length(channel) BETWEEN 1 AND 20),
  ts          TEXT NOT NULL CHECK (length(ts) >= 1),
  emoji       TEXT NOT NULL CHECK (length(emoji) BETWEEN 1 AND 100),
  queued_at   TEXT NOT NULL,
  reacted_at  TEXT,
  UNIQUE (channel, ts, emoji)
) STRICT;
INSERT INTO history_tables (name, no_delete) VALUES ('slack_reactions', 1);
-- Posts merged before the upgrade still get their reaction.
INSERT INTO slack_reactions (channel, ts, emoji, queued_at, reacted_at)
  SELECT p.channel, p.ts, p.reaction, r.merged_at, p.reacted_at
    FROM pr_posts p JOIN prs r ON r.plan = p.plan AND r.key = p.pr
   WHERE p.reaction IS NOT NULL AND r.status = 'merged'
   ORDER BY r.merged_at, p.plan, p.pr
  ON CONFLICT DO NOTHING;
