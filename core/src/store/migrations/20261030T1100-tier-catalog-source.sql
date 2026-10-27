-- cpd-migration: breaking after 58
-- pro-d26 (Plan bifrost, PR catfp): `tier_catalog` is keyed by the content of `<agentDir>/models.json` too: `models_sha`, the lowercase
-- hex SHA-256 of its bytes, `''` when the file does not exist. The primary key is (pi_version, models_sha, letter, model).
-- It is a cache, so the table is rebuilt EMPTY: the rows are recomputed by the next `cpd tiers`. `history_tables` still lists
-- `tier_catalog`, so the history triggers and `tier_catalog_no_delete` are derived again (K4), as `blue-tier` rebuilt `inbox_items`.
CREATE TABLE tier_catalog_new (
  pi_version TEXT NOT NULL,
  models_sha TEXT NOT NULL,
  letter     TEXT NOT NULL CHECK (letter IN ('O', 'S', 'H')),
  model      TEXT NOT NULL,
  efforts    TEXT NOT NULL CHECK (json_valid(efforts) AND json_type(efforts) = 'array'),
  read_at    TEXT NOT NULL,
  PRIMARY KEY (pi_version, models_sha, letter, model)
) STRICT;
DROP TABLE tier_catalog;
ALTER TABLE tier_catalog_new RENAME TO tier_catalog;
