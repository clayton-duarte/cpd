-- cpd-migration: additive
-- Dynamic session titles (contract §2.2 Session, §3.7 E10, §4.5 "Named
-- migration session-titles"; hs-d257): the title an auxiliary agent writes
-- for a Lead or a Manager (`cpd session retitle`) and when it was written,
-- both null until the first one. The reconcile (K4) regenerates `sessions`'
-- history triggers from the new columns.
ALTER TABLE sessions ADD COLUMN title TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 60);
ALTER TABLE sessions ADD COLUMN titled_at TEXT;

-- The Event kind `session-titled` {title} (contract §5.2).
INSERT INTO event_kinds (name) VALUES ('session-titled');
