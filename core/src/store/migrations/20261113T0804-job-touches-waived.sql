-- cpd-migration: additive
-- `jobs.touches_waived` (b69): the declared `touches` paths the Lead waived as unchanged (`cpd job approve-untouched`).
-- Its own field: only the unchanged check reads it, never the outside-the-touches check (`touches_approved`).
ALTER TABLE jobs ADD COLUMN touches_waived TEXT CHECK (touches_waived IS NULL OR (json_valid(touches_waived) AND json_type(touches_waived) = 'array'));
