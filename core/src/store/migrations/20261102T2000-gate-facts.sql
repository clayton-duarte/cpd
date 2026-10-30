-- cpd-migration: additive
ALTER TABLE attempts ADD COLUMN gate_hook TEXT CHECK (gate_hook IN ('pre-commit', 'pre-merge'));
ALTER TABLE attempts ADD COLUMN gate_command TEXT;
ALTER TABLE attempts ADD COLUMN gate_exit INTEGER;
