-- cpd-migration: additive
-- The information a Lead adds to a Job after its Spec is locked (pro-d294; contract §3.4 "Addenda", §4.4): append-only, in no fingerprint.
CREATE TABLE job_addenda (
  plan       TEXT NOT NULL,
  job        TEXT NOT NULL,
  number     INTEGER NOT NULL CHECK (number >= 1),
  attempt    TEXT,
  session    TEXT REFERENCES sessions(id),
  text       TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 20000),
  created_at TEXT NOT NULL,
  PRIMARY KEY (plan, job, number),
  FOREIGN KEY (plan, job) REFERENCES jobs(plan, key),
  FOREIGN KEY (plan, attempt) REFERENCES attempts(plan, id),
  CHECK ((attempt IS NULL) = (session IS NULL))
) STRICT;
CREATE TRIGGER job_addenda_no_update BEFORE UPDATE ON job_addenda BEGIN SELECT RAISE(ABORT, 'append_only: job_addenda'); END;
INSERT INTO history_tables (name, no_delete) VALUES ('job_addenda', 1);
INSERT INTO event_kinds (name) VALUES ('job-addendum');
