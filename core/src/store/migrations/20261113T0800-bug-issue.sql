-- cpd-migration: additive
-- bug-issue (pro-d587): `bug_records.issue`, the GitHub issue of a bug (https://github.com/<owner>/<repo>/issues/<N>),
-- null until the bug is mitigated; and `workflows.bug_id`, the bug a Workflow fixes, null for a Workflow of no bug.
-- The open bugs get their issue from 20261113T0801-bug-issue-backfill.
ALTER TABLE bug_records ADD COLUMN issue TEXT;
ALTER TABLE workflows ADD COLUMN bug_id TEXT REFERENCES bug_records(id);
