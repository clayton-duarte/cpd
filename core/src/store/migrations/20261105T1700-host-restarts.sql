-- cpd-migration: additive
-- host-restarts (pro-d430, contract 6.19): the durable host crash-restart bound (3 restarts in 10 minutes) kept in the database, so it survives an engine restart. host_window_start is a time, NULL when no window is open.
ALTER TABLE sessions ADD COLUMN host_restarts INTEGER NOT NULL DEFAULT 0 CHECK (host_restarts BETWEEN 0 AND 3);
ALTER TABLE sessions ADD COLUMN host_window_start TEXT;
