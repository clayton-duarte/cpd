-- cpd-migration: additive
-- pr-archived (pro-d392): GitHub's `isArchived` of the PR's repository, read by the direct read. An archived open PR is read once and not polled again.
ALTER TABLE pull_requests ADD COLUMN archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1));
