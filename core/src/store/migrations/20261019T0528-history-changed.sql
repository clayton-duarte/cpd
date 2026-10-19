-- cpd-migration: breaking after 17
-- History `update` rows hold the key and only the changed columns (contract
-- §4.4 "History row shape", §4.5; hs-d234, Plan quality finding sql-F2): a Job
-- status flip stored its whole row twice, about 15 KB.
--
-- It drops the `_hist_upd` trigger of each table of `history_tables`; the
-- runner's reconcile (§4.5 K4), right after it in the same transaction,
-- creates them again in the new shape (a migration never creates a generated
-- trigger). `IF EXISTS`: on a fresh store the lookup tables of `kind-tables`
-- get their triggers only from that same reconcile.
--
-- No row is rewritten: the `update` rows already in `history` keep their whole
-- `before` and `after` (the table is append-only).
--
-- Breaking (A1): an older binary's schema check compares every generated
-- trigger with its own generator, which makes the whole-row shape, so it would
-- find these triggers changed and refuse to write (`store_tampered`).

DROP TRIGGER IF EXISTS contexts_hist_upd;
DROP TRIGGER IF EXISTS context_records_hist_upd;
DROP TRIGGER IF EXISTS plans_hist_upd;
DROP TRIGGER IF EXISTS pull_requests_hist_upd;
DROP TRIGGER IF EXISTS prs_hist_upd;
DROP TRIGGER IF EXISTS pr_needs_hist_upd;
DROP TRIGGER IF EXISTS plan_reviews_hist_upd;
DROP TRIGGER IF EXISTS jobs_hist_upd;
DROP TRIGGER IF EXISTS dependencies_hist_upd;
DROP TRIGGER IF EXISTS attempts_hist_upd;
DROP TRIGGER IF EXISTS escalations_hist_upd;
DROP TRIGGER IF EXISTS sessions_hist_upd;
DROP TRIGGER IF EXISTS inbox_items_hist_upd;
DROP TRIGGER IF EXISTS tier_catalog_hist_upd;
DROP TRIGGER IF EXISTS event_kinds_hist_upd;
DROP TRIGGER IF EXISTS record_kinds_hist_upd;
DROP TRIGGER IF EXISTS history_tables_hist_upd;
