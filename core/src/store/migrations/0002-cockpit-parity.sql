-- Migration 2 (contract §4.5, cockpit parity): what `pr poll` last read from the PR.
-- The three columns are written together; the binary keeps them all null or all set.
ALTER TABLE plans ADD COLUMN pr_title TEXT CHECK (pr_title IS NULL OR length(pr_title) >= 1);
ALTER TABLE plans ADD COLUMN pr_review_decision TEXT CHECK (pr_review_decision IN ('none', 'approved', 'changes_requested'));
ALTER TABLE plans ADD COLUMN pr_checks TEXT CHECK (pr_checks IN ('none', 'passed', 'pending', 'failed'));
