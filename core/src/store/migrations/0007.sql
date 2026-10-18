-- Migration 7 (contract §4.5, Plan manager B1, hs-a1): Context records live only
-- in the database, so `contexts.sync_failing` goes. SQLite refuses to drop a
-- column a trigger names, so the history triggers of `contexts` are dropped here
-- and generated again by the migration's code, which then imports the old stores.
DROP TRIGGER contexts_hist_ins;
DROP TRIGGER contexts_hist_upd;
DROP TRIGGER contexts_no_delete;
ALTER TABLE contexts DROP COLUMN sync_failing;
