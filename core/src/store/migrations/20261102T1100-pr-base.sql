-- cpd-migration: additive
ALTER TABLE pull_requests ADD COLUMN base_ref TEXT;
ALTER TABLE pull_requests ADD COLUMN default_branch TEXT;
