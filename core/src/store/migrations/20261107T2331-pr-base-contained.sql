-- cpd-migration: additive
-- pr-base-contained (Plan `merge-uptodate`, contract §2.2 "Ready to merge", §7.6): the persisted containment flag.
-- `1` when the pull request's head contains the current default-branch head, `0` when it does not, NULL when unknown.
-- Nullable, no default and no data change: a row polled before this migration has NULL, which is "not ready" (no
-- silent assumption); the poll sets and refreshes it for every open pull request whose `base_ref` is its
-- `default_branch`, and leaves it NULL for a stacked pull request, which is not subject to the rule.
ALTER TABLE pull_requests ADD COLUMN base_contained INTEGER;
