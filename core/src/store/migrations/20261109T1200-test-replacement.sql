-- cpd-migration: breaking after 126
-- A replacement intent fences the source before any external stop and records the
-- starting commit without changing the original Job or its Attempt refs.
CREATE TABLE test_replacements (
  plan TEXT NOT NULL,
  source TEXT NOT NULL,
  new_key TEXT NOT NULL,
  request TEXT NOT NULL CHECK (json_valid(request)),
  state TEXT NOT NULL CHECK (state IN ('pending', 'complete', 'abandoned')),
  start_sha TEXT,
  created_txn INTEGER NOT NULL REFERENCES txns(id),
  PRIMARY KEY (plan, source),
  UNIQUE (plan, new_key),
  FOREIGN KEY (plan, source) REFERENCES jobs(plan, key)
) STRICT;
INSERT INTO history_tables (name, no_delete) VALUES ('test_replacements', 0);
DROP TRIGGER dependencies_no_delete;
CREATE TRIGGER dependencies_no_delete BEFORE DELETE ON dependencies
WHEN (SELECT status FROM jobs WHERE plan = OLD.plan AND key = OLD.job) <> 'draft'
 AND NOT ((SELECT deleted_at FROM plans WHERE id = OLD.plan) IS NOT NULL)
 AND NOT ((SELECT command FROM txns WHERE id = cpd_txn()) = 'job replace-test' AND EXISTS (SELECT 1 FROM test_replacements r WHERE r.plan = OLD.plan AND r.source = OLD.needs AND r.state = 'pending' AND r.created_txn <> cpd_txn()))
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;
DROP TRIGGER dependencies_locked;
CREATE TRIGGER dependencies_locked BEFORE INSERT ON dependencies
WHEN (SELECT status FROM jobs WHERE plan = NEW.plan AND key = NEW.job) <> 'draft'
 AND (SELECT created_txn FROM jobs WHERE plan = NEW.plan AND key = NEW.job) <> cpd_txn()
 AND NOT ((SELECT command FROM txns WHERE id = cpd_txn()) = 'job replace-test' AND EXISTS (SELECT 1 FROM test_replacements r WHERE r.plan = NEW.plan AND r.new_key = NEW.needs AND r.state = 'pending' AND r.created_txn <> cpd_txn()))
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;
CREATE TRIGGER test_replacements_no_delete BEFORE DELETE ON test_replacements
WHEN (SELECT deleted_at FROM plans WHERE id = OLD.plan) IS NULL
BEGIN SELECT RAISE(ABORT, 'append_only: test_replacements'); END;
