-- cpd-migration: additive
-- `jobs.touches_approved` (contract §2.2, §7.5, hs-n641): the repo-relative paths the Lead approved a Job to change
-- beyond its Spec `touches`, a JSON array, null until `job approve-touches`. Not Locked and in no fingerprint: it
-- changes after the lock. Additive: one nullable `ADD COLUMN` with a `CHECK`, like `job-resources`; the history
-- triggers of `jobs` are derived (K4) and regenerated with the new column. It rewrites no row.
ALTER TABLE jobs ADD COLUMN touches_approved TEXT CHECK (touches_approved IS NULL OR (json_valid(touches_approved) AND json_type(touches_approved) = 'array'));
