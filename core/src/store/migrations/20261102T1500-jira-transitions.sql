-- cpd-migration: additive
-- The Jira transition queue (pro-d126, pro-d128; Plan jira-sync; contract §4.4 `jira_transitions`): one row per PR and target,
-- PENDING, SETTLED or PARKED. A data table: no history triggers, no row written here.
CREATE TABLE jira_transitions (
  id           INTEGER PRIMARY KEY,
  plan         TEXT NOT NULL,
  pr           TEXT NOT NULL,
  target       TEXT NOT NULL CHECK (target IN ('in_progress','in_review','done')),
  queued_at    TEXT NOT NULL,
  settled_at   TEXT,
  outcome      TEXT CHECK (outcome IN ('moved','unchanged','skipped')),
  refused_at   TEXT,
  refusal      TEXT,
  refused_with TEXT,
  UNIQUE (plan, pr, target),
  FOREIGN KEY (plan, pr) REFERENCES prs(plan, key),
  CHECK ((settled_at IS NULL) = (outcome IS NULL)),
  CHECK ((refused_at IS NULL) = (refusal IS NULL) AND (refused_at IS NULL) = (refused_with IS NULL)),
  CHECK (settled_at IS NULL OR refused_at IS NULL)
) STRICT;
