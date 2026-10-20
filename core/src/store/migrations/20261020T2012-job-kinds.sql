-- cpd-migration: breaking after 28
-- Job kinds and the PR's shape (contract §2.2 "Job kinds", §4.4, §4.5 `job-kinds`, hs-d340, hs-d341, hs-d497).
-- Breaking: a binary that does not know it would schedule an `open` Job as an ordinary Job, run a `test` Job without
-- its red check and an `impl` Job without its guard, and one `CREATE TRIGGER` on `jobs` is breaking by A1 anyway.
-- It rewrites no row: every existing Job is `work` with the other columns null, every existing PR is `free`. The
-- history triggers of `jobs` and `prs` are derived (K4): the runner's reconcile regenerates them with the new columns.

ALTER TABLE jobs ADD COLUMN kind TEXT NOT NULL DEFAULT 'work' CHECK (kind IN ('work', 'docs', 'test', 'impl', 'open', 'update'));
ALTER TABLE jobs ADD COLUMN tests TEXT CHECK (tests IS NULL OR (json_valid(tests) AND json_type(tests) = 'array' AND json_array_length(tests) >= 1));
ALTER TABLE jobs ADD COLUMN test_files TEXT CHECK (test_files IS NULL OR (json_valid(test_files) AND json_type(test_files) = 'array' AND json_array_length(test_files) >= 1));
ALTER TABLE jobs ADD COLUMN test_approved TEXT CHECK (test_approved IS NULL OR (json_valid(test_approved) AND json_type(test_approved) = 'array'));
ALTER TABLE prs ADD COLUMN shape TEXT NOT NULL DEFAULT 'free' CHECK (shape IN ('tdd', 'docs', 'config', 'free'));

-- The shape is the PR's identity, written once by `pr add` and never changed, in any phase.
CREATE TRIGGER prs_shape_fixed BEFORE UPDATE OF shape ON prs
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;

-- `jobs_locked` with `kind`, `tests` and `test_files` in its column list.
DROP TRIGGER jobs_locked;
CREATE TRIGGER jobs_locked BEFORE UPDATE OF title, tier, goal, context, touches, acceptance, out_of_scope, plan, key, pr, review, kind, tests, test_files ON jobs
WHEN OLD.status <> 'draft'
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;

-- A kind's Spec fields, its approvals and the review flag agree with it. Triggers, not CHECKs: ALTER TABLE cannot add
-- a check across columns. The same rule on insert and on update.
CREATE TRIGGER jobs_kind_spec_ins BEFORE INSERT ON jobs
WHEN (NEW.kind <> 'test' AND (NEW.tests IS NOT NULL OR NEW.test_files IS NOT NULL))
  OR ((NEW.tests IS NULL) <> (NEW.test_files IS NULL))
  OR (NEW.kind = 'test' AND NEW.tests IS NULL AND NEW.status NOT IN ('draft', 'cancelled'))
  OR (NEW.kind <> 'impl' AND NEW.test_approved IS NOT NULL)
  OR (NEW.review = 1 AND NEW.kind <> 'work')
BEGIN SELECT RAISE(ABORT, 'spec_invalid'); END;

CREATE TRIGGER jobs_kind_spec_upd BEFORE UPDATE OF kind, tests, test_files, test_approved, review, status ON jobs
WHEN (NEW.kind <> 'test' AND (NEW.tests IS NOT NULL OR NEW.test_files IS NOT NULL))
  OR ((NEW.tests IS NULL) <> (NEW.test_files IS NULL))
  OR (NEW.kind = 'test' AND NEW.tests IS NULL AND NEW.status NOT IN ('draft', 'cancelled'))
  OR (NEW.kind <> 'impl' AND NEW.test_approved IS NOT NULL)
  OR (NEW.review = 1 AND NEW.kind <> 'work')
BEGIN SELECT RAISE(ABORT, 'spec_invalid'); END;
