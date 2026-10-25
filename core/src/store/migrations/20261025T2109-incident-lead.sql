-- cpd-migration: additive
-- The Session the incident pill opens (hs-d657, hs-d678): `cpd incident start` stores the first
-- `--plan` Plan's `lead_session`, and a pre-migration row falls back to its alphabetically first
-- Plan's Lead at click time.
ALTER TABLE incident ADD COLUMN lead TEXT REFERENCES sessions(id);
