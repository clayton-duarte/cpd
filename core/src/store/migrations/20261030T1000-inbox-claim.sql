-- cpd-migration: additive
-- pro-d58 (C2): a Manager claims a `review-requested` Inbox item; `claimed_at` records when, and the Event kind `inbox-claimed`
-- names it. A claim, like a dismissal, lasts for the pull request: closed items are kept, so E8 reads the history.
ALTER TABLE inbox_items ADD COLUMN claimed_at TEXT CHECK (claimed_at IS NULL OR kind = 'review-requested');
INSERT INTO event_kinds (name) VALUES ('inbox-claimed');
