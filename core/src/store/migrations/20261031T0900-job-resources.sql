-- cpd-migration: breaking after 60
-- The optional Spec field `resources` (contract §2.2, commands.md §5.1): the names of the repo's declared exclusive
-- resources a Job takes, a JSON array of one or more names, null for a Job that takes none. Locked with the Job and
-- in no fingerprint. Breaking: one `CREATE TRIGGER` on `jobs` is breaking by A1; the history triggers of `jobs` are
-- derived (K4) and regenerated with the new column. It rewrites no row.

ALTER TABLE jobs ADD COLUMN resources TEXT CHECK (resources IS NULL OR (json_valid(resources) AND json_type(resources) = 'array' AND json_array_length(resources) >= 1));

-- `jobs_locked` as pro-d79's migration `job-edit-queued` made it (a queued Job with no Attempt may change its Spec),
-- with `resources` added to its column list and nothing else changed.
DROP TRIGGER jobs_locked;
CREATE TRIGGER jobs_locked BEFORE UPDATE OF title, tier, goal, context, touches, acceptance, out_of_scope, plan, key, pr, review, kind, tests, test_files, resources ON jobs
WHEN OLD.status <> 'draft'
 AND NOT (OLD.status = 'queued'
          AND OLD.kind NOT IN ('open', 'update')
          AND NOT EXISTS (SELECT 1 FROM attempts WHERE plan = OLD.plan AND job = OLD.key)
          AND NEW.title IS OLD.title AND NEW.tier IS OLD.tier AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key
          AND NEW.pr IS OLD.pr AND NEW.review IS OLD.review AND NEW.kind IS OLD.kind)
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;
