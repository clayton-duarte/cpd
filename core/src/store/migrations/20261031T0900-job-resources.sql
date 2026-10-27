-- cpd-migration: breaking after 56
-- The optional Spec field `resources` (contract §2.2, commands.md §5.1): the names of the repo's declared exclusive
-- resources a Job takes, a JSON array of one or more names, null for a Job that takes none. Locked with the Job and
-- in no fingerprint. Breaking: one `CREATE TRIGGER` on `jobs` is breaking by A1; the history triggers of `jobs` are
-- derived (K4) and regenerated with the new column. It rewrites no row.

ALTER TABLE jobs ADD COLUMN resources TEXT CHECK (resources IS NULL OR (json_valid(resources) AND json_type(resources) = 'array' AND json_array_length(resources) >= 1));

-- `jobs_locked` with `resources` in its column list.
DROP TRIGGER jobs_locked;
CREATE TRIGGER jobs_locked BEFORE UPDATE OF title, tier, goal, context, touches, acceptance, out_of_scope, plan, key, pr, review, kind, tests, test_files, resources ON jobs
WHEN OLD.status <> 'draft'
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;
