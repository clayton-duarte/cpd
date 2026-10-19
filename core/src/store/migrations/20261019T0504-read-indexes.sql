-- cpd-migration: additive
-- The indexes the cockpit, the commands and the engine's liveness read need
-- (contract §4.4; Plan quality findings sql-F4, tui-F3, sql-F6, engine-F2).
-- Only indexes on existing tables: additive (A1), nothing is rewritten.

-- sql-F4, tui-F3: the job counts of every cockpit refresh (`plan_heads`) read
-- `jobs` by Plan and status; without this they scan the table, and a Job row's
-- long text columns (overflow pages) come before `status`.
CREATE INDEX jobs_plan_status ON jobs(plan, status);

-- sql-F6: the engine read of every widgets refresh, `watch --ensure` and
-- `events follow --kind` select Events by `kind`.
CREATE INDEX events_kind ON events(kind, id);

-- engine-F2: E2's liveness read (core/src/watch/liveness.rs) wants the Sessions
-- that are not `stopped` or still have a pane; `sessions` only grows. The index
-- holds those rows only. SQLite uses a partial index when the query's WHERE
-- implies the index's, which it proves for an identical expression: keep this
-- WHERE and E2's in step (a unit test of liveness.rs pins it). `<> 'stopped'`
-- rather than a list of the other states, so a state added later by a breaking
-- migration is listed without editing this.
CREATE INDEX sessions_live ON sessions(id) WHERE state <> 'stopped' OR tmux_pane IS NOT NULL;
