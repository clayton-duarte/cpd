-- cpd-migration: additive
-- pr-light-cache (pro-d453): what the Light read of the poll carries and no column held: the pull request's author login
-- (`classify` needs the real author), the `latestReviews` nodes with their commit oid, and the label names (the policy hold).
-- Nullable, no default, no data change: a row not polled since has NULL, which is "unknown".
ALTER TABLE pull_requests ADD COLUMN author_login TEXT;
ALTER TABLE pull_requests ADD COLUMN reviews_json TEXT;
ALTER TABLE pull_requests ADD COLUMN labels_json TEXT;
