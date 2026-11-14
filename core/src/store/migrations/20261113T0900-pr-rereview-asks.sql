-- cpd-migration: additive
-- The bot re-review comment (pro-d588, b71): one row per (plan, workflow, head_sha, bot) the engine asked to re-review, so a rerun of
-- the poll posts nothing new. A data row: it appends no Event.
CREATE TABLE pr_rereview_asks (
  plan      TEXT NOT NULL,
  workflow  TEXT NOT NULL,
  head_sha  TEXT NOT NULL CHECK (length(head_sha) >= 1),
  bot       TEXT NOT NULL CHECK (length(bot) >= 1),
  asked_at  TEXT NOT NULL,
  PRIMARY KEY (plan, workflow, head_sha, bot)
) STRICT;
