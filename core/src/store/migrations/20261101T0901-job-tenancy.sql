-- cpd-migration: breaking after 66
-- Resource tenancy by queue (pro-d106; contract §3.4 "Tenancy", §4.4): `job_pools`, the view `job_tenants` and `jobs.skipped_n`.
CREATE TABLE job_pools (
  plan  TEXT NOT NULL,
  job   TEXT NOT NULL,
  pool  TEXT NOT NULL CHECK (length(pool) BETWEEN 1 AND 200 AND pool GLOB '[a-z]*:*'),
  PRIMARY KEY (plan, job, pool),
  FOREIGN KEY (plan, job) REFERENCES jobs(plan, key)
) STRICT;
CREATE TRIGGER job_pools_first_attempt BEFORE INSERT ON job_pools
WHEN NOT EXISTS (SELECT 1 FROM attempts WHERE plan = NEW.plan AND job = NEW.job)
  OR EXISTS (SELECT 1 FROM attempts WHERE plan = NEW.plan AND job = NEW.job AND number > 1)
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;
CREATE TRIGGER job_pools_no_update BEFORE UPDATE ON job_pools BEGIN SELECT RAISE(ABORT, 'append_only: job_pools'); END;
ALTER TABLE jobs ADD COLUMN skipped_n INTEGER CHECK (skipped_n IS NULL OR skipped_n >= 1);
CREATE UNIQUE INDEX jobs_skipped_n ON jobs(skipped_n) WHERE skipped_n IS NOT NULL;
CREATE VIEW job_tenants AS
  SELECT j.plan AS plan, j.key AS job, p.pool AS pool
    FROM jobs j JOIN job_pools p ON p.plan = j.plan AND p.job = j.key
   WHERE j.status NOT IN ('merged', 'cancelled')
      OR EXISTS (SELECT 1 FROM attempts a WHERE a.plan = j.plan AND a.job = j.key AND a.status <> 'ended')
  UNION ALL
  SELECT j.plan, j.key, 'workers'
    FROM jobs j
   WHERE EXISTS (SELECT 1 FROM attempts a WHERE a.plan = j.plan AND a.job = j.key)
     AND (j.status NOT IN ('merged', 'cancelled')
          OR EXISTS (SELECT 1 FROM attempts a WHERE a.plan = j.plan AND a.job = j.key AND a.status <> 'ended'));
INSERT INTO history_tables (name, no_delete) VALUES ('job_pools', 1);
