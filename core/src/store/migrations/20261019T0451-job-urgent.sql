-- cpd-migration: additive
-- The urgent flag of a Job (contract §2.2, §3.7 E3; User, Context record
-- "Urgent Jobs", hs-d238): 0 or 1, default 0. Not a Locked field and in no
-- fingerprint. The history triggers of `jobs` are derived (K4): the runner's
-- reconcile regenerates them with the new column.

ALTER TABLE jobs ADD COLUMN urgent INTEGER NOT NULL DEFAULT 0 CHECK (urgent IN (0, 1));
