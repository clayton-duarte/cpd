-- cpd-migration: additive
-- workflow-prereqs: external prerequisites for workflows (LaunchDarkly flags, manual tasks)
CREATE TABLE workflow_prereqs (
  plan         TEXT NOT NULL,
  workflow     TEXT NOT NULL,
  id           INTEGER NOT NULL,
  kind         TEXT NOT NULL CHECK (kind IN ('ld-flag', 'manual')),
  details_json TEXT NOT NULL CHECK (json_valid(details_json)),
  met_at       TEXT,
  met_by       TEXT,
  PRIMARY KEY (plan, workflow, id),
  FOREIGN KEY (plan, workflow) REFERENCES workflows(plan, key)
) STRICT;
