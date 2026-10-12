# Job `watch`: `cpd watch` scheduler and `cpd plan lock`

Tier S4 · Needs: `ledger-attempts`, `herdr`, `queue-ask`, `pr` · Read `_common.md` first. Contract sections: §9, §11 (`plan lock`, `watch`, `job cancel` running case), §12 (the parts `watch` owns: spawning `attempt run`, timeouts, cancellation, crash detection), §10.

## Goal
The long-running loop that turns a locked plan into running Attempts within the machine's concurrency cap. It enforces timeouts (excluding Escalation waits), mirrors Escalations into the Ledger, handles cancellation and crashes, polls the PR, and stops when the Workflow is done. Also the `plan lock` command that starts Dispatch.

## Files you own
`src/dispatch/watch.ts`, `src/dispatch/schedule.ts`, `src/dispatch/liveness.ts`, `src/dispatch/plan-lock.ts`, `src/commands/{watch,plan-lock}.ts`, `test/dispatch/{watch,schedule,liveness,plan-lock}.test.ts`, `test/commands/{watch,plan-lock}.test.ts`.

## Requirements
- `plan-lock.ts`, `cpd plan lock [--no-watch]`:
  - `lockPlan` (`ledger-core`);
  - `createLeadBranch` (`git-wt`) and record `leadWorktree`;
  - unless `--no-watch`, split a pane from the Lead pane (`workflow.json.lead.paneId`, `--no-focus`, cwd = Lead worktree) and run `cpd watch --wf <wf>` in it with `herdr pane run`. This is a pane with no agent; typing a command into a fresh shell pane is fine.
  - Output per §11.
- `watch.ts`, `cpd watch --wf <wf>`:
  - Single live instance via `watch.json` (pid + heartbeat). Refuse `watch_running` if another live pid holds it; take over a stale one.
  - Tick every `tickSeconds` through the steps in §11, each a pure function over a snapshot, so ticks are testable with fakes.
  - Exit 0 when the phase is `done`.
  - Survive transient tool errors: log them, send one `error` message per distinct error per 10 minutes, and keep ticking. Exit on config or Ledger corruption, after an `error` message.
- `schedule.ts`:
  - Ready Jobs in creation order; slot acquisition (`slots.ts`);
  - `startAttempt` (CAS) at the Tier given by `attemptTier`, or `retry_<n>_tier` when set;
  - spawn `cpd attempt run <attempt-id> --wf <wf> --slot <i>` detached (`spawn` with `detached: true`, `stdio` to `logs/attempt-<id>.log`, then `unref`);
  - release the slot if the spawn fails.
- `liveness.ts`, all deterministic with an injectable clock:
  - **Active time** = now − `started_at` − Σ(time each Escalation of the Attempt stayed open).
  - **Timeout:** close the Worker pane and set write-once `timed_out`.
  - **Crash:** the Worker agent is gone and the `attempt run` pid is not alive, so fail the Attempt as `crashed` (ladder) and send its message.
  - **Cancellation:** `cancel_requested` → stop the Attempt as §11 describes.
  - **Escalations:** mirror new questions and answers into the Ledger (`escalations.ts` of `ledger-attempts`); expire orphaned questions (`expireOrphans`).
- Completion: send `all-merged` exactly once (write-once Workflow metadata).
- PR phase: call the `pr` poll function every `prPollSeconds`.

## Acceptance
1. Pure tick tests with fakes:
   - cap 2 with 5 ready Jobs starts exactly 2; when one finishes, the next starts on the following tick;
   - Dependencies respected (a Job whose blocker is not Merged never starts);
   - an Escalation open for 10 minutes extends the timeout by 10 minutes;
   - a timeout closes the pane and marks `timed_out`;
   - a crash fails the Attempt and requeues it via the ladder;
   - `cancel_requested` stops the Attempt and cancels the Job;
   - `all-merged` is sent once;
   - PR polling cadence follows `prPollSeconds`.
2. A second `cpd watch` for the same Workflow while one is alive gives `watch_running`; with a stale `watch.json` it takes over.
3. `plan lock` integration with real `bd` + git + wt and a fake herdr: refuses without approval; with approval, Jobs are queued, the Lead Branch worktree exists, and the pane split + run argv was recorded.
4. A machine-wide cap across two Workflows sharing the same `CPD_HOME` (two watch ticks against fakes): the total running Attempts never exceed `concurrency`.

## Out of scope
The body of `attempt run` (Job `attempt-run`); PR internals (Job `pr`).
