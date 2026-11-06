-- cpd-migration: additive
-- codereview-followup (pro-d416): a codereview Plan knows its PR (`pr_subject`, `pr:<repo>#<n>`) and when changes were requested; an inbox item records the head and the reviewed commit.
ALTER TABLE plans ADD COLUMN pr_subject TEXT;
ALTER TABLE plans ADD COLUMN changes_requested_at TEXT;
ALTER TABLE inbox_items ADD COLUMN head_oid TEXT;
ALTER TABLE inbox_items ADD COLUMN reviewed_oid TEXT;
