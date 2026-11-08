-- cpd-migration: additive
-- pr-merge-armed (commands.md §9.10): `pr merge` found the head behind the default branch, refused `head_behind_main`
-- and queued the Workflow's update Job; the poll merges automatically once the checks pass on a head that contains the
-- default branch again. `NOT NULL DEFAULT 0`, cleared when the pull request merges, closes or is cancelled.
ALTER TABLE pull_requests ADD COLUMN merge_armed INTEGER NOT NULL DEFAULT 0 CHECK (merge_armed IN (0, 1));
