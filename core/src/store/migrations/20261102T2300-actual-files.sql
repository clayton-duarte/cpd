-- cpd-migration: additive
-- actual_files holds the paths `attempt submit` records for the Attempt, a JSON array of strings.
-- NULL means not captured: before this migration, or never submitted; [] is a submit with no changed path.
ALTER TABLE attempts ADD COLUMN actual_files TEXT CHECK (actual_files IS NULL OR (json_valid(actual_files) AND json_type(actual_files) = 'array'));
