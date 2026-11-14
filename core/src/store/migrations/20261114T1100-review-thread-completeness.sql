-- cpd-migration: breaking after 144
-- Older writers cannot maintain the completeness flag when they replace open_threads.
-- Old empty arrays do not establish that GitHub returned a complete connection.
-- The next complete read supplies the trusted thread set.
ALTER TABLE pull_requests ADD COLUMN review_threads_complete INTEGER NOT NULL DEFAULT 0 CHECK (review_threads_complete IN (0, 1));

-- A bug-specific owner request bypasses the mitigation grace period.
ALTER TABLE bug_records ADD COLUMN escalated_at TEXT;
ALTER TABLE bug_records ADD COLUMN escalated_by TEXT;
ALTER TABLE bug_records ADD COLUMN escalation_reason TEXT;
