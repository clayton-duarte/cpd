-- cpd-migration: additive
-- swr-deploys-etags (docs/architecture/swr-github.md §8.1; pro-d563; G11):
-- ETag list cache for conditional deploys workflow polling.
CREATE TABLE IF NOT EXISTS swr_deploy_lists (
  repo         TEXT NOT NULL,
  workflow     TEXT NOT NULL,
  viewer_login TEXT NOT NULL,
  etag         TEXT NOT NULL,
  PRIMARY KEY (repo, workflow)
) STRICT;
