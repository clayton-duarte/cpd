-- cpd-migration: additive
ALTER TABLE sessions ADD COLUMN runner TEXT NOT NULL DEFAULT 'pi' CHECK (runner IN ('pi', 'durable'));
