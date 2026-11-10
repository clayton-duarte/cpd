-- cpd-migration: additive
-- workflow-repair (pro-d465, contract §4.4, §7.6 "The repair marker"): `workflows.repair`, null when no `pr rebase` or `pr push` of
-- the Workflow runs, else the JSON `{"pid", "start", "command"}` of that repair. Update Job dispatch reads it. Additive: every
-- existing Workflow keeps null, which means no repair runs.
ALTER TABLE workflows ADD COLUMN repair TEXT CHECK (repair IS NULL OR json_valid(repair));
