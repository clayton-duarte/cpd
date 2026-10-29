-- cpd-migration: additive
ALTER TABLE attempts ADD COLUMN runner TEXT NOT NULL DEFAULT 'pi' CHECK (runner IN ('pi', 'durable'));
ALTER TABLE attempts ADD COLUMN worker_pid INTEGER;
