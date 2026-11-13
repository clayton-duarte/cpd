-- cpd-migration: additive
-- Codex findings on #1256 (pro-d588, b71): `open_threads` is the JSON array of the review thread ids GitHub still shows unresolved at
-- the last comments read (a thread answered by `cpd pr reply` is in it), `reviewers` the JSON array of the logins that authored review
-- comments, comments or reviews, `rereview_head` the head the bot re-review pass last saw with a configured bot in `reviewers`.
-- Nullable, no data change: NULL = not read since this migration (the gate falls back to `feedback`; the re-review pass asks nothing).
ALTER TABLE pull_requests ADD COLUMN open_threads TEXT;
ALTER TABLE pull_requests ADD COLUMN reviewers TEXT;
ALTER TABLE pull_requests ADD COLUMN rereview_head TEXT;
