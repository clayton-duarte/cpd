-- cpd-migration: breaking after 66
-- Resource tenancy by queue (pro-d106; contract §3.4 "Tenancy", §4.4): `job_pools`, the view `job_tenants` and `jobs.skipped_n`.
-- STUB of the test Job `tenancy-tests`: the table and the column exist, the view is EMPTY and the triggers are missing, so the
-- tests of the tenancy fail on assertions. The implementing Job replaces the view with the join of the contract and adds
-- `job_pools_first_attempt` and `job_pools_no_update`.
CREATE TABLE job_pools (
  plan  TEXT NOT NULL,
  job   TEXT NOT NULL,
  pool  TEXT NOT NULL CHECK (length(pool) BETWEEN 1 AND 200 AND pool GLOB '[a-z]*:*'),
  PRIMARY KEY (plan, job, pool),
  FOREIGN KEY (plan, job) REFERENCES jobs(plan, key)
) STRICT;
ALTER TABLE jobs ADD COLUMN skipped_n INTEGER CHECK (skipped_n IS NULL OR skipped_n >= 1);
CREATE UNIQUE INDEX jobs_skipped_n ON jobs(skipped_n) WHERE skipped_n IS NOT NULL;
CREATE VIEW job_tenants AS SELECT plan, key AS job, 'workers' AS pool FROM jobs WHERE 0;
INSERT INTO history_tables (name, no_delete) VALUES ('job_pools', 1);
