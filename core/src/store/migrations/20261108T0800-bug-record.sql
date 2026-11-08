-- cpd-migration: additive
-- The `bug_records` table (contract §2.2 "Bug record", §4.5 `bug-record`; air-d103, air-d104, air-d107): the bug
-- report protocol (commands.md §33) as records, not prose. A new table: an older binary ignores it and reads the
-- store unchanged. A bug is reported `open` with no mitigation and no fix, a mitigation fills `mitigation`,
-- `mitigated_by` and `mitigated_at`, and a close fills `root_fix`, `regression_test`, `closed_at` and `closed_by`.
CREATE TABLE bug_records (
  id              TEXT PRIMARY KEY CHECK (id GLOB 'b[0-9]*' AND length(id) BETWEEN 2 AND 12),
  plan            TEXT REFERENCES plans(id),
  title           TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
  symptom         TEXT NOT NULL CHECK (length(symptom) BETWEEN 1 AND 20000),
  cause           TEXT CHECK (cause IS NULL OR length(cause) BETWEEN 1 AND 20000),
  mitigation      TEXT CHECK (mitigation IS NULL OR length(mitigation) BETWEEN 1 AND 20000),
  mitigated_by    TEXT,
  mitigated_at    TEXT,
  root_fix        TEXT CHECK (root_fix IS NULL OR length(root_fix) BETWEEN 1 AND 200),
  regression_test TEXT CHECK (regression_test IS NULL OR length(regression_test) BETWEEN 1 AND 200),
  status          TEXT NOT NULL CHECK (status IN ('open', 'closed')),
  reported_by     TEXT,
  reported_at     TEXT NOT NULL,
  closed_at       TEXT,
  closed_by       TEXT,
  -- A mitigation sets its text and when together; the recorder is null outside a Session (contract §2.2).
  CHECK ((mitigation IS NULL) = (mitigated_at IS NULL)),
  CHECK (mitigated_by IS NULL OR mitigation IS NOT NULL),
  -- A closed bug has its root fix and regression test recorded, and when it closed; an open one has none of them.
  CHECK ((status = 'closed') = (closed_at IS NOT NULL)),
  CHECK (status <> 'closed' OR (root_fix IS NOT NULL AND regression_test IS NOT NULL))
) STRICT;
INSERT INTO history_tables (name, no_delete) VALUES ('bug_records', 1);
