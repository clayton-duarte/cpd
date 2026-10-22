-- cpd-migration: additive
-- By-user Sessions (hs-d540, hs-d542; commands.md §3.10, §4.1, §10.8): 1 for a Lead or Manager the User created from the
-- tree, which runs no turn and holds no first-message file until the User's first prompt. Existing rows get 0, today's
-- behaviour. Rewrites no row. The reconcile (K4) regenerates `sessions`' history triggers from the new column.
ALTER TABLE sessions ADD COLUMN by_user INTEGER NOT NULL DEFAULT 0 CHECK (by_user IN (0, 1));
