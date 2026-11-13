-- cpd-migration: additive
CREATE TABLE refactor_coverage_baselines (
  plan TEXT NOT NULL,
  workflow TEXT NOT NULL,
  base_sha TEXT NOT NULL,
  report TEXT NOT NULL CHECK (json_valid(report)),
  PRIMARY KEY (plan, workflow),
  FOREIGN KEY (plan, workflow) REFERENCES workflows(plan, key)
) STRICT;
