-- cpd-migration: breaking after 104
-- drop-question-answer (pro-d8): physically delete every question and answer Context record with its
-- curation and retirement rows. The generated delete guards are dropped for the DELETEs; the runner's
-- reconcile recreates them after the last migration. The hist_del trigger writes the history delete rows.
DROP TRIGGER IF EXISTS context_records_no_delete;
DROP TRIGGER IF EXISTS record_curations_no_delete;
DROP TRIGGER IF EXISTS record_retirements_no_delete;
DROP TRIGGER IF EXISTS inbox_items_no_delete;
-- An Inbox row of any status may name a deleted record; a left row would fail every Inbox read.
DELETE FROM inbox_items
 WHERE record IN (SELECT id FROM context_records WHERE kind IN ('question', 'answer'));
DELETE FROM record_curations
 WHERE (context, record) IN (SELECT context, id FROM context_records WHERE kind IN ('question', 'answer'));
DELETE FROM record_retirements
 WHERE (context, record) IN (SELECT context, id FROM context_records WHERE kind IN ('question', 'answer'))
    OR (context, target) IN (SELECT context, id FROM context_records WHERE kind IN ('question', 'answer'));
DELETE FROM context_records WHERE kind IN ('question', 'answer');
