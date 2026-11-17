-- cpd-migration: additive
-- Observations belong to the update Job, and remain unknown for older Jobs.
ALTER TABLE jobs ADD COLUMN observed_head_oid TEXT;
ALTER TABLE jobs ADD COLUMN observed_base_oid TEXT;
