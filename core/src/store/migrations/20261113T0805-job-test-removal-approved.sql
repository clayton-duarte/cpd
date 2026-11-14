-- cpd-migration: additive
-- `jobs.test_removal_approved` (contract §2.2, §7.5; b70): the test case names the Lead approved a Job to remove or weaken,
-- a JSON array, null until `job approve-test-removal`. Not Locked and in no fingerprint: it changes after the lock.
-- Additive: one nullable `ADD COLUMN` with a `CHECK`, like `touches_approved`; the history triggers of `jobs` are
-- derived (K4) and regenerated with the new column. It rewrites no row.
ALTER TABLE jobs ADD COLUMN test_removal_approved TEXT CHECK (test_removal_approved IS NULL OR (json_valid(test_removal_approved) AND json_type(test_removal_approved) = 'array'));
