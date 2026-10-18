# Quality research: the engine and the Attempt lifecycle

Job `engine` of Plan `quality` (hs-d153 order: Work, Fast, Good; hs-d157 measurement rules). Area: `core/src/{watch,driver,merge,attempt,ask,git,host,metrics,spec}/`, contract §3, §6, §7, commands §5.5, §6, §8, §12. Read at `main` `e8ad70d`. This file changes no code.

**Summary.** The engine is cheap and steady: its whole tick of SQL costs about 0.4 ms, the live engine holds 12–14 MB RSS with 8–9 fds and does not grow (N=12 samples, 110 s; HWM equals RSS after 44 min), and a driver is 11 MB and 8 fds. The merge, the ladder, the locks and the per-repo lock are well built and well tested (34 merge tests). What is not right is at the edges. **Work:** a commit made after `attempt submit` is merged without any check, because the merge never compares the Attempt's HEAD with `submitted_sha` (reproduced); a failed driver spawn counts as a Worker crash and burned the whole retry ladder of six Jobs in 10 ms each (observed); E2's and R's window kill is tried once, R3 never, so a Worker's pi can outlive its Attempt; git, tmux, node and the engine's poll child have no deadline while the engine is single-threaded; the engine's real stop path is SIGHUP, which it does not handle, so `watch-stopped` was never appended (14 starts, 0 stops); a reload between the nudge and the second idle disables `no_submit`. **Fast:** nothing in the engine's own tick needs speed work; the measured costs that exist are each driver's 2 Hz `tmux` fork (about 0.9% of a core per driver) and the PR poll's fork (about 145 ms of child CPU per poll, one poll every 10 s, not 5 s). **Good:** the ledger code is written twice (`driver/ledger.rs`, `merge/ledger.rs`), the A9 window kill three times, the flock helper nine times.

## All findings

| id | class | what | size |
|---|---|---|---|
| engine-W1 | Work | A commit made after `attempt submit` is merged unchecked: the merge never compares HEAD with `submitted_sha`, and the Worker's pi runs until A9 | Job |
| engine-W2 | Work | A failed driver spawn ends the Attempt `crashed`, a ladder cause: it burns `retry_cap` and climbs the Tier with no Worker ever started | Job |
| engine-W3 | Work | A9's window kill is retried only in the driver; E2 and R2 kill once, R3 never: the Worker's pi outlives an ended Attempt | Job |
| engine-W4 | Work | No deadline on git, tmux, node or the engine's `cpd pr poll` child; E6 and E7 run inline in the one-thread tick | Job |
| engine-W5 | Work | The engine handles SIGTERM and SIGINT but its real stop is SIGHUP (`kill-window`): `watch-stopped` was never appended (14 starts, 0 stops) | Job |
| engine-W6 | Work | The nudge's turn count lives in a local variable: a driver reload after the nudge disables `no_submit`, the Attempt ends `timed_out` instead | Job |
| engine-W7 | Work | E5 runs `cpd` from `PATH`, not the engine's recorded executable (the 00:02Z incident printed `plan_required`) | Job |
| engine-W8 | Work | One tmux hiccup is "the pane is gone": it crashes a live Worker's Attempt (driver) and stops every Session (E2) | Job |
| engine-W9 | Work | The PR poll runs every 10 s, the documented 5 s (720 an hour); measured 360 | Job |
| engine-W10 | Work | `watch --ensure` is not idempotent under concurrency: the loser reports `watch_start_failed` while the engine runs (unverified) | Job |
| engine-W11 | Work | A review Job's unbounded wait for its Lead counts against `attempt_minutes` and holds a slot (unverified end to end) | Job |
| engine-W12 | Work | A hook's output is held in memory and its log written only when the hook ends | Job |
| engine-F1 | Fast | Every driver forks `tmux list-panes -a` twice a second: 90% of a driver's CPU | Job |
| engine-F2 | Fast | E2's Session query scans every Session ever made: 82 µs at 194 rows, 12.4 ms at 9 700 | Job |
| engine-F3 | Fast | `cpd pr poll` costs one fork and about 145 ms of child CPU per poll (measured, no new action) | – |
| engine-G1 | Good | `driver/ledger.rs` and `merge/ledger.rs` duplicate the ladder, the end transaction, the cancel read and the error Event | Job |
| engine-G2 | Good | Three A9 window kills, two driver-lock probes and nine flock sites | Job |
| engine-G3 | Good | Driver logs have no timestamps; the engine's log has | Job |
| engine-G4 | Good | `Driver::wait` (120 lines) holds the idle/nudge decision where only a real tmux and a fake pi can test it | Job |
| engine-G5 | Good | `host/pi.rs` (931 lines) holds four concerns | Job |
| engine-G6 | Good | `Report` never forgets a failing object (a bounded, tiny leak under hs-d34) | Job |

## Work

### engine-W1 — a commit after submit is merged without a check

- **What.** `attempt submit` records `submitted_sha` after the quick check and the tamper check. The merge (A8) never reads it: step 4's tamper check reads the branch tip as it is when the merge starts, and step 8 merges `rev-parse HEAD` of the Attempt worktree as it is at step 8. The Worker's pi keeps running until A9 kills its window (after the merge), and the driver waits for the per-repo lock without a limit (`RepoLock::wait`), so there is a window of any length in which the Worker can commit again. Whatever it commits in that window is merged without the quick check; and a protected-path change committed between step 4 and step 8 is merged without the tamper check.
- **Where.** `core/src/merge/mod.rs:71` (`RepoLock::wait`), `:135` (tamper check), `:151` (`new = rev-parse HEAD`); `core/src/attempt/submit.rs` (the only writer of `submitted_sha`).
- **Evidence.**
  - `grep -rn submitted_sha core/src | grep -v "attempt/"` prints only `store/migrations/0001.sql` (the CHECKs) and `job/show.rs` (display): no reader in `merge/` or `driver/`.
  - Reproduced with a throw-away copy of `cli/tests/merge.rs` plus one test (submit, then commit `late.txt` in the Attempt worktree, then `merge::merge`), deleted afterwards: `scripts/cargo.sh test -p cpd --test merge_probe probe_a_commit -- --nocapture` printed `PROBE submitted=e3beed87… late=4e7ab204… result=Merged { sha: "4e7ab204…" } lead_now=4e7ab204…` and `test result: ok. 1 passed`. So the Lead Branch moved to a commit that is not the submitted one and no check ran on it. The existing test `tampered_when_the_diff_touches_a_protected_path` (`cli/tests/merge.rs:471`) also commits after the `submitted` fixture and relies on the merge-time check, which is why the protected case is caught only when the commit lands before step 4.
  - The race between step 4 and step 8 is from the code (no test): unverified as a run.
- **Impact.** The Gate's promise ("a Merged Attempt passed its quick check and touched no protected path") holds only for a Worker that obeys "Stop now". The spec calls submit the Worker's one write (commands §6.1).
- **Proposed fix.** Amend contract §7.4 first (hs-d4), then at step 2 compare `rev-parse refs/heads/<attempt branch>` with `submitted_sha` and end the Attempt `failed` (a non-ladder cause, or the existing `Tampered`) when they differ; and kill the Worker's window when the driver sees `merging` (before it waits for the repo lock), so nothing can move HEAD afterwards. Test: the probe above, turned into a regression test in `cli/tests/merge.rs`; it fails today (the merge succeeds).
- **Size.** Job.
- **Overlap.** `cross-repo` PR `plumbing` edits the same function (`steps`, step 5, hs-d139: skip the rebase when the Attempt contains `old`); the fix is at steps 2 and 8 and does not collide, but land after it.

### engine-W2 — a spawn failure is a Worker crash on the ladder

- **What.** `driver::start` inserts the Attempt first and spawns the driver second. If the spawn fails it ends the Attempt `failed` / `crashed` ("as E2 would"). `crashed` is a ladder cause, so the Job goes back to `queued` at the next Tier, E3 starts it again at the next tick, and a persistent host fault (a missing executable, EMFILE, ENOMEM) walks the whole ladder in milliseconds. No Worker ever ran.
- **Where.** `core/src/driver/start.rs:106-124` (the spawn-failure arm), `core/src/driver/ledger.rs:239` (`next_tier`) and `:136` (`Cause::is_ladder`).
- **Evidence** (the copy `$T/cpd.db`, taken from `~/.cpd/cpd.db`):
  - `select count(*), min(round((julianday(ended_at)-julianday(started_at))*86400000)), max(...) from attempts where cause='crashed' and (julianday(ended_at)-julianday(started_at))*86400 < 1` → `12|2.0|10.0`: twelve Attempts lived 2–10 ms.
  - They are the a2 and a3 of six Jobs: a2 at Tiers `S4`, `S4`, `S4`, `O4`, `S3`, `S3` and a3 at `S5`, `S5`, `S5`, `O5`, `S4`, `S4` (`select plan,id,tier,status,outcome,cause,started_at,ended_at from attempts where cause in ('crashed',…)`), and `select kind,count(*) from events where kind='job-failed' and json_extract(data,'$.cause')='crashed'` → `6`: six Jobs failed.
  - `~/.cpd/run/watch.log` lines 338–343: `E3 (scheduling) failed for Job sccache-cargo of Plan dev-speed: Cannot start the driver of Attempt sccache-cargo-a2: cannot run /home/clayton/.local/bin/cpd (deleted)… (spawn_failed)`, six of them at 00:02:19Z.
- **Impact.** Every host fault costs the Jobs their retry budget and a more expensive Tier, and ends in `job-failed` that a Lead must `job retry`. That is the part of hs-n79/hs-n81 that the install guards do not touch.
- **Proposed fix.** A spawn failure is not an Attempt's outcome. Either end it with a non-ladder cause (`setup_failed` already exists and is non-ladder) or roll the insert back and leave the Job `queued` with the `error` Event E3 already appends; contract §3.4 says "ends it `failed` / `crashed`", so amend it first. Test: `cli/tests/driver.rs`, a `job dispatch` whose exe cannot run, then assert the Job is not `queued` at a higher Tier and the attempt count is 1. Fails today.
- **Size.** Job.
- **Overlap.** hs-n79, hs-n81, hs-d151, hs-d152 (Plan pi-pin, merged as `e8ad70d`): `host::exe::started_as()` and the install guards remove the `(deleted)` trigger (confirmed in `driver/start.rs`); the class "any other spawn failure burns the ladder" is not covered.

### engine-W3 — the Worker's window is killed once, or never, outside the driver

- **What.** Contract A9: "`kill-window` is tried 3 times, 1 s apart, before the `error` Event". Only the driver does so (`worker::kill_window` with `retry_kill`). E2's `end_dead` and R2's `rest_of_a9` call `kill_window` once and return the error with `?`; R3 (the merge did not happen) never touches tmux. In all three the Attempt is already `ended`, and E2 scans only `status <> 'ended'`, so nothing retries. A submitted Worker's pi is idle but alive, so the leak is a whole pi process (and its `events follow` child) until the Plan is `done`. In R2 the `?` also skips `remove_merged_attempt`, so the merged Attempt's worktree stays too.
- **Where.** `core/src/watch/liveness.rs:151`, `core/src/merge/recover.rs:95-109` (R3 arm, no kill), `:212-224` (`rest_of_a9`, single kill, `?`), versus `core/src/driver/worker.rs:324` (`kill_window` with retry).
- **Evidence.** Code reading plus `grep -rn kill_window core/src | grep -v "fn kill_window"` (liveness 151/325, recover 222, ensure 45, worker 334, poll 290); a probe of R on a submitted Attempt with no driver (the same throw-away test file) printed `PROBE R3 recovered=[Some(Failed { cause: Crashed, … })] … worktree_exists=true branch_exists=true`. The window leak itself is **unverified as a run**: the harness fixture has no tmux window (not run: it needs the live Worker flow).
- **Impact.** A driver that dies during a merge (an install at the wrong moment, hs-n81; a reboot; OOM) leaves a live pi per such Attempt for the Plan's life; hs-d34 ("no orphaned processes") is the policy.
- **Proposed fix.** One shared function (`driver/worker.rs::kill_window`) called from `end_dead`, `rest_of_a9` and an R3 arm; per-item best effort so a failed kill does not skip the worktree removal. Test: `cli/tests/watch.rs`, a fake `tmux` that fails the first `kill-window`, E2 ends a dead driver, the window is gone after the tick; same for a `merging` Attempt (R3).
- **Size.** Job.
- **Overlap.** `docs/research/teardown.md` G12 (fixed in the driver only: the retry exists in `worker.rs`); hs-n27 / hs-n35 name the R-path lock file only.

### engine-W4 — no deadline on git, tmux, node or the poll child, and E6/E7 run in the tick

- **What.** The engine is one thread: E1–E7 run in turn, and a signal is looked at only between steps. Calls without a deadline: every `git` (`git/run.rs`), every `tmux` (`host/tmux.rs`), `node` for the catalog, the Context store's git (`context/git.rs`), and the engine's own `cpd pr poll` child (`Command::output()`). A hook has a deadline (`setup_minutes`, `gate_minutes`), `gh` has one (60 s, poll 15 s), the store has `store_busy_ms`. E7 starts a PR inside the tick: `git fetch`, `ls-remote`, `worktree add` and the `pre-start` hook (up to 15 minutes); E6 runs `context sync`, whose lock `ContextLock::acquire` is a blocking `flock` with no timeout. One hung `git fetch` (a network black hole; `GIT_TERMINAL_PROMPT=0` does not stop an ssh or TCP stall) stops E2 (timeouts, cancels, dead drivers), E3 and the SIGTERM handling for as long as it hangs.
- **Where.** `core/src/git/run.rs:42`, `core/src/host/tmux.rs:243`, `core/src/config/catalog.rs:144`, `core/src/context/git.rs:102`, `core/src/watch/pr.rs:94` (the `.output()` sites); `core/src/watch/engine.rs:106` (`start_prs`), `core/src/watch/contexts.rs:34`, `core/src/context/lock.rs:31`; the merge pushes under the per-repo lock (`git/worktree.rs` `push_lead_branch`), so a hung push also holds that lock for every driver of the repo.
- **Evidence.** `grep -rn "\.output()\|\.status()\|\.spawn()\|wait_with_output" core/src/{watch,driver,merge,attempt,ask,git,host,metrics,spec} core/src/context core/src/pr/gh.rs core/src/config/catalog.rs` lists the sites above; only `gh.rs` and `exe.rs` (`VERSION_TIMEOUT`) and `hooks/exec.rs` have a deadline. Typical inline cost when nothing hangs, measured: `git ls-remote --heads origin main` against the real origin, N=10, median 530 ms, max 557 ms; this repo's `pre-start` (`npm ci --no-audit --no-fund`) N=3, 0.58/0.66/0.67 s. A hang was **not** reproduced (a run needs a stalled network; forbidden on the live engine): unverified.
- **Impact.** Bounded and small on a good day (a PR start stalls the engine about 2 s); unbounded on a bad one. The one thing the engine exists to do (E2) stops.
- **Proposed fix.** One `run_with_deadline` used by `git::run` and `Tmux::output` (kill the child at the deadline, `git_failed`/`tmux_failed` with `timeout` in details), a deadline on the poll child above `POLL_TIMEOUT` (for example 60 s), and `flock(LOCK_NB)` with a retry bound in `ContextLock`. Moving E6/E7 to their own thread is the larger alternative (Plan). Test: a fake `git` first on `PATH` that sleeps 30 s, `git::run::run` with a 1 s deadline returns `git_failed` in about 1 s.
- **Size.** Job (deadlines); Plan if E6/E7 move off the tick.
- **Overlap.** `docs/research/gh-interface.md` failure 2 (the poll's `gh` timeout, since done as `POLL_TIMEOUT`); the `manager` branch deletes E6 (`watch/contexts.rs`), which removes that part; hs-d107 (the poll stays on `gh`).

### engine-W5 — the engine's normal stop is SIGHUP, which it does not handle

- **What.** `catch_signals` handles SIGTERM and SIGINT; commands §12 and contract §3.7 say the engine appends `watch-stopped` when it stops. But `scripts/install-cpd.sh` restarts the engine with `tmux kill-window -t =cpd-work:watch`, and tmux sends SIGHUP. The default SIGHUP action ends the process in mid-tick with no `watch-stopped` and no log line. The graceful path has never run in production.
- **Where.** `core/src/watch/engine.rs:165-173` (`catch_signals`), `:211-222` (the stop path); `scripts/install-cpd.sh:73`.
- **Evidence.** In the copy: `select kind,count(*) from events where kind like 'watch-%' group by 1` → `watch-started|13`, no `watch-stopped` row. In the live log (read-only): `grep -c "watch: started (pid" ~/.cpd/run/watch.log` → `14`; `grep -c "stopping on a signal" ~/.cpd/run/watch.log` → `0`. During this research the engine was replaced (pid 589549 gone, pid 1557271 started 01:32:34Z): the log has no stop line before the new `started`.
- **Impact.** The Events and the log never say the engine stopped, so `doctor` and the Lead cannot tell a restart from a crash; a SIGHUP in the middle of E3 (after the Attempt insert, before the spawn) leaves a `starting` Attempt with a free lock, which the next engine ends `crashed` on the ladder (W2's cost). From code, not reproduced.
- **Proposed fix.** Add SIGHUP to `catch_signals` (one line) so the loop ends at its next check and writes `watch-stopped`; test in `cli/tests/watch.rs` next to `watch_holds_the_lock_refuses_a_second_and_stops_on_sigterm`: SIGHUP → exit 0, one `watch-stopped`. Fails today.
- **Size.** Job (about 20 minutes).
- **Overlap.** `docs/research/teardown.md` §2 notes "a killed engine writes no `watch-stopped` (not a leak)" and leaves it; no Plan covers it.

### engine-W6 — the nudge baseline is lost when the driver reloads

- **What.** A5 nudges once, then ends `no_submit` if the Worker goes idle again "after a nudge". The driver decides the second idle with `turns > nudged_at`, where `nudged_at` is a local `Option<u64>` set when it nudges. After a reload (`exec` of the new binary, the routine path of hs-d33/hs-d38) the new driver starts with `nudged_at = None` and the Attempt's `nudged = true`; at its first idle sample it sets the baseline to the *current* turn count (`get_or_insert(turns)`), and an idle Worker produces no more turns, so `turns > baseline` is never true. The Attempt then waits for `attempt_minutes` (90 here) and ends `timed_out`, which is not a ladder cause (the Job fails instead of retrying at the next Tier).
- **Where.** `core/src/driver/run.rs:360` (`nudged_at`), `:423` and `:427` (set and compare).
- **Evidence.** Code reading only: **unverified as a run** (it needs a nudged Worker and an install in the right minutes). `grep -n nudge cli/tests/driver_reload.rs` prints nothing: the four reload tests do not cover it; `an_idle_worker_is_nudged_once_then_ends_no_submit` (`cli/tests/driver.rs:521`) runs without a reload.
- **Impact.** Any install between the nudge and the Worker's second idle (minutes) disables `no_submit` for that Attempt; installs happen several times a day with up to six drivers.
- **Proposed fix.** Put the turn count in the `attempt-nudge` Event's `data` (it already carries `prompt`) and have a resumed driver read it (contract §3.4 already says the nudge-once state comes from that Event). Test in `cli/tests/driver_reload.rs`: nudge, reload, idle again → `no_submit`. Fails today (inferred from the code).
- **Size.** Job.
- **Overlap.** Plan pi-pin (driver reload) touched `reload_if_installed` only; not this.

### engine-W7 — E5 starts `cpd` from `PATH`, not the engine's executable

- **What.** E3 starts drivers from the path recorded at start (`host::exe::started_as`, hs-d151). E5 starts `cpd pr poll` as `Command::new("cpd")` (`session::PROGRAM`), resolved on the tmux server's `PATH`. After an install that replaces `cpd` the engine's poll runs whichever `cpd` is first on that `PATH`, possibly another version than the engine.
- **Where.** `core/src/watch/pr.rs:84-94`; contrast `core/src/driver/start.rs` (`started_as`).
- **Evidence.** The copy has one `error` Event `plan_required|watch` and the log line `2026-10-18T00:02:19.486Z cpd watch: E5 (PR) failed for the machine: No Plan selected: pass --plan <id> or set CPD_PLAN. (plan_required)`, in the same second as the six `spawn_failed` lines of W2: the engine ran a `cpd` that required `--plan` for `pr poll` (I infer, not verified: the schema-5 build of hs-n81 was first on `PATH`). Query: `select json_extract(data,'$.code'),json_extract(data,'$.source'),count(*) from events where kind='error' group by 1,2`.
- **Impact.** Mixed versions inside one engine during every install window; here one error, no data loss.
- **Proposed fix.** Use `started_as()` in `watch/pr.rs`; test beside `e3_starts_the_driver_after_the_engines_binary_is_replaced` (`cli/tests/watch.rs:677`).
- **Size.** Job (10 minutes).
- **Overlap.** hs-d151 names "the engine starts Attempt drivers from the recorded path" only; E5 is not listed.

### engine-W8 — one tmux hiccup is read as "the pane is gone"

- **What.** `Tmux::list_panes` returns an empty list when `list-panes` fails and `list-sessions` then also exits non-zero (`server_running()` false). Two callers read an empty list as "every pane is gone": the driver's A4 loop ends the Attempt `crashed` and kills its (alive) window, and E2's `sessions` marks every Session with a pane `stopped` and clears its `tmux_pane`. A failed `fork` of `tmux`, or a server that does not answer one connect, is indistinguishable from "no server".
- **Where.** `core/src/host/tmux.rs:438-444`, `core/src/driver/run.rs:364` (a `list_panes()?` error leaves `wait`, and `driver.fail` ends the Attempt `crashed`) and `:399-407` (pane missing: `crashed`), `core/src/watch/liveness.rs` (`sessions`).
- **Evidence.** Code reading; **unverified as a run** (tmux was not made to fail: forbidden on the live server, not attempted on a private one).
- **Impact.** A transient failure costs an hour of Worker progress and a ladder step; or flips all live Leads to `stopped`. Rare, and expensive when it happens.
- **Proposed fix.** `list_panes` returns an error unless tmux says "no server running" (stderr text or exit status plus socket absence); callers retry once before they decide a pane is gone. Test with a fake `tmux` that fails one call.
- **Size.** Job.
- **Overlap.** none.

### engine-W9 — the PR poll runs every 10 s, not 5 s

- **What.** E5 polls `pr_poll_seconds` after the *end* of the previous poll, but it only looks at tick boundaries (every `tick_seconds` = 5). A poll takes about 1.3 s, so at the next tick only 3.7 s have passed and E5 waits for the one after: the period is 10 s. Contract §3.7, `config.toml` and hs-d96 say 5 s ("720 an hour"; `docs/research/gh-interface.md` budgets 720 points).
- **Where.** `core/src/watch/pr.rs:28-33`.
- **Evidence.** Read-only observation of the live engine (pid 589549), 75 s, the engine's children polled every 20 ms from `/proc/<pid>/task/*/children`: `cpd pr poll` first seen at offsets 1.06, 11.06, 21.05, 31.05, 41.05, 51.05, 61.05, 71.05 s with lifetimes 1.38, 1.48, 1.30, 1.81, 1.05, 1.03, 1.33, 1.20 s (N=8): a period of 10.0 s, a median duration of 1.3 s.
- **Impact.** Halves the poll budget (good for the rate limit), doubles the latency of "PR merged" detection against what the contract says. Either the number or the documents are wrong.
- **Proposed fix.** Decide with the User: document 10 s (and say the period is a multiple of the tick), or make the wait between ticks end at the poll's due time. Test: `cli/tests/watch.rs` `e5_polls_every_pr_poll_seconds_…` with a poll that takes 1 s and a tick of 5.
- **Size.** Job.
- **Overlap.** hs-d96 / hs-d107 (Plan pr-poller) set the 5 s; the code merged with this drift.

### engine-W10 — `watch --ensure` is not idempotent under concurrency

- **What.** `ensure` probes the lock, then creates the window. Two concurrent calls (`plan lock`, `cpd tui` and the install script all call it) both see the lock free; the second finds the session and runs `new-window`, so a second engine starts, takes no lock (exit 3 `watch_running`), and its pane dies; `wait_started` then fails the second caller with `watch_start_failed` although an engine runs. commands §12 says "Idempotent: yes".
- **Where.** `core/src/watch/ensure.rs:28-52`, `:60-89`.
- **Evidence.** Code reading; **unverified** (running `watch --ensure` is outside this Job's rules).
- **Impact.** A spurious `watch_start_failed` for `plan lock` ("the Plan stays locked", commands §4.8) and a dead `watch` window until the next ensure.
- **Proposed fix.** After `wait_started` fails, re-probe the lock: held means success (`started: false, already: true`), and kill the dead window. Test with two `ensure` processes started together.
- **Size.** Job.
- **Overlap.** none.

### engine-W11 — a review Job's wait counts against `attempt_minutes`

- **What.** `active_time` is "now minus `started_at` minus open-Escalation time", and `started_at` is set when the `starting` Attempt is inserted. A review Job's driver then waits for its Lead to be idle "without a limit" (`wait_for_lead`) before A3. When the Attempt turns `running`, E2 has already counted the whole wait: a wait over `attempt_minutes` makes the first E2 sample set `stop_request = timed_out` (non-ladder, the Job fails). E2 never limits the wait itself (`stop_request` returns early unless `status == "running"`), and the Attempt holds one of the `concurrency` slots meanwhile.
- **Where.** `core/src/watch/liveness.rs:186` and `active_time`; `core/src/driver/worker.rs:223-245`.
- **Evidence.** Code reading; **unverified end to end**.
- **Impact.** Small today (a Lead is usually idle in seconds, `fork_quiet_seconds = 5`); a Lead blocked on a User dialog for hours turns it into a failed Job.
- **Proposed fix.** Count active time from the `running` transition (the `attempt-status` Event time), or pause the clock while `starting`; and bound `wait_for_lead` by a `timeouts` key or report an `error` after N minutes. Test: unit-test `active_time`.
- **Size.** Job.
- **Overlap.** none.

### engine-W12 — hook output is held in memory and the log is written at the end

- **What.** `exec::execute` collects every line of every hook command into a `Vec<String>`; `run_hook` writes `run/<plan>/logs/<attempt>.<hook>.log` once, after the hook ends. A hook that prints a lot grows the driver's (or `attempt submit`'s) memory without bound; a hook killed at its deadline, or a driver that dies during `pre-start`, leaves no log at all; nobody can `tail -f` a 30-minute quick check.
- **Where.** `core/src/git/hooks/exec.rs:71`, `:118`; `core/src/git/hooks/mod.rs:143`.
- **Evidence.** Code reading. The biggest existing hook logs are 199 KB (`ls -l ~/.cpd/run/*/logs/*.log`: N=329, median 27 B, max 199 477 B, sum 2.0 MB), so no harm so far; a hostile or runaway command is **unverified**.
- **Impact.** Low now; the log is the only evidence of a failed A2 or quick check.
- **Proposed fix.** Open the log first and write each line as the reader thread gets it; keep only the last 60 lines in memory (`TAIL_LINES`). Test in `cli/tests/git.rs`: a hook that prints then sleeps, the log has the line before the hook ends.
- **Size.** Job.
- **Overlap.** none.

*Known and already tracked, verified here:* the driver lock file is left behind by R (and by E2's probe of an Attempt that ended meanwhile, because `merge::take_free_driver_lock` opens with `create`): a probe of R2 and R3 listed `attempt-pay--two-a1.lock` and `attempt-pay--three-a1.lock` in `locks/` afterwards. Covered by hs-n27 / hs-n35 (deferred teardown fixes); not re-proposed. Base merges dropped by the rebase: hs-n52, hs-n67, hs-d139 (in `cross-repo` PR `plumbing`).

## Fast

All numbers are from this machine, 2026-10-18. SQL ran on the copy (`sqlite3` library 3.45.1 through Python's `sqlite3`, read-only, N=200 each, first call dropped); live processes were only read from `/proc`.

### The engine tick: SQL and how often each step runs

| Step | Runs | What it costs | SQL median / max (N=200) | Plan (`EXPLAIN QUERY PLAN`) |
|---|---|---|---|---|
| E1 | every tick (5 s) | 1 query, one `kill(pid, 0)` per open Escalation | 14 / 106 µs (0 rows) | scan `escalations` by pk index |
| E2 attempts | every tick | 1 query; per Attempt a lock probe (open, `flock`); per running one `escalations` query (5 / 11 µs) | 68 / 80 µs (6 rows) | scan `attempts` by pk (183 rows) |
| E2 sessions | every tick | 1 query and one `tmux list-panes -a` fork (3.57 ms, below) | 82 / 113 µs (16 rows of 194) | scan `sessions` by pk (F2) |
| E3 | every tick | READY query and `count(*)` of active Attempts; the catalog only when a Job will start | 78 / 96 µs (4 rows); 6 / 11 µs | `attempts_active` covering index; scan `j` |
| E4 | every tick | 1 query | 14 / 26 µs | scan `prs` by pk |
| E5 | `pr_poll_seconds` after the previous poll ends, seen only at ticks: every 10 s (W9) | a `cpd pr poll` fork, 1.3 s | – | – |
| E6 | every 300 s | `git` per Context store | – | – |
| E7 | every tick | 1 query (STARTABLE) | 10 / 60 µs | scan `prs` |
| Driver A4 | every 500 ms per driver | `list-panes` fork, `ledger::load` join (11 / 1 541 µs), Session state (6 / 13 µs) | – | pk lookups |

One tick of SQL: about 0.36 ms of the 5 s. `Store::read` opens no new connection per query. The catalog is cached in the DB: `cpd tiers` (reads it) N=15 median 5 ms, max 7 ms; `cpd --version` median 2.0 ms.

### Live memory, descriptors, CPU (read-only, `ps`, `/proc/<pid>/status`, `/proc/<pid>/fd`, `/proc/<pid>/stat`)

| Process | RSS / HWM | fds | threads | CPU (self / children) |
|---|---|---|---|---|
| `cpd watch` (pid 589549, up 43.7 min) | 13.4 MB / 13.4 MB; then N=12 samples at 10 s: 14 004 kB flat | 9 (3 are the pane's tty, 2 `cpd.db`, wal, shm, `watch.log`, `watch.lock`) | 7 (1 + one `child.wait()` reaper per live driver: 6) | 0.07% / 2.98% since start (includes the hooks' CPU of drivers it reaped) |
| `cpd watch` (pid 1557271, after the restart) | 11.9 MB / 11.9 MB | 8 | 1 | window 120 s, N=1: 0.07% / 1.45% (1.74 s of 12 poll cycles and 24 `list-panes`: about 145 ms per poll cycle) |
| 6 live drivers (`attempt run`, ages 5–11 min; mine included) | 11.1–11.5 MB (N=6) | 8 each (0–2, lock fd 3, `cpd.db`, wal, shm, the log) | 1 | 0.10% self, 0.74–1.01% children each (N=5: one driver had exited before the CPU read; one snapshot each) |

No leak is visible in the 110 s sample or the 44 min HWM; the fd sets are the expected ones.

### `run/` logs per Attempt (`du`, `ls -l`, read-only)

`du -sh ~/.cpd/run` → 11 MB for 20 Plans (largest Plan 1.3 MB). Driver logs `run/*/logs/attempt-*.log`: N=185, median 278 B, p90 284 B, max 432 B, sum 48 KB. Hook logs: see W12. `run/watch.log`: 59 580 B, 445 lines since 2026-10-15. Nothing to fix; a retention rule is hs-d63.

### engine-F1 — each driver forks `tmux list-panes -a` twice a second

- **What.** A4 loops every `POLL` = 500 ms: `tmux list-panes -a` (a fork and exec), `ledger::load`, and when idle the Session file parse. The tmux fork is nearly all the driver's CPU.
- **Where.** `core/src/driver/run.rs:33` (`POLL`), `:364`.
- **Evidence.** Private tmux server (`TMUX_TMPDIR=$(mktemp -d)`, socket `quality-eng-a1-…`, 31 panes, killed afterwards): `list-panes -a -F <the liveness format>` N=50: median 3.57 ms wall, p95 4.59 ms, max 6.21 ms, 2.54 ms CPU per call; `list-sessions` N=50 median 3.21 ms. Live drivers: self CPU 0.10%, children 0.74–1.01% each (5 drivers, one snapshot). 2.54 ms × 2/s = 0.5% plus the fork cost: consistent. Six drivers: about 5% of a core and 12 forks a second, to learn that one pane is alive.
- **Impact.** Small in absolute terms; it is the dominant steady-state CPU of the lifecycle and it is the same call that W8 makes fragile.
- **Proposed fix.** Record the pane's pid (`pane_pid`) once and test it with `kill(pid, 0)` and `/proc` (no fork); call `list-panes` only on the exit path for the status. Or poll at 2 s. Measure again: expect the children CPU to go near zero.
- **Size.** Job.
- **Overlap.** none.

### engine-F2 — E2's Session query scans every Session ever made

- **What.** `SELECT … FROM sessions WHERE state <> 'stopped' OR tmux_pane IS NOT NULL` cannot use an index; `sessions` only grows (`sessions_no_delete`), one row per Worker plus Leads.
- **Where.** `core/src/watch/liveness.rs:276`.
- **Evidence.** `EXPLAIN QUERY PLAN` → `SCAN sessions USING INDEX sqlite_autoindex_sessions_1`. Scaled on a scratch copy of the copy (triggers dropped, 49 clones of every Session with state `stopped` and no pane; `$T/scale.db`): 194 rows → 82 µs median / 113 µs max; 9 700 rows → 12 429 µs median / 18 623 µs max (N=100). The Attempts query could not be scaled (the table's CHECKs refuse synthetic ids): unverified extrapolation.
- **Impact.** Negligible now (0.25% of a tick at 9 700 Sessions, about five months of history at the observed 65 Sessions a day: 194 in 3 days); it grows without bound.
- **Proposed fix.** A partial index `ON sessions(id) WHERE state <> 'stopped' OR tmux_pane IS NOT NULL` in a migration; test: the plan shows `USING INDEX`. Belongs with the store Job's migration (`store-sql`).
- **Size.** Job.
- **Overlap.** Area `store-sql`; migrations 0007–0009 belong to Plan `manager`.

### engine-F3 — `cpd pr poll`: measured, nothing new proposed

Each poll is a `cpd` process and a `gh` process: 1.3 s median wall (N=8, W9's observation), about 145 ms of child CPU per poll cycle (window above, N=1, includes two `list-panes`), gh 34 MB RSS per call per `docs/research/gh-interface.md`. The pr-poller Plan compared `gh` with a direct API and the User chose `gh` (hs-d107); not re-proposed.

## Good

### engine-G1 — the ledger code exists twice

- **What.** `driver/ledger.rs` (276 lines) and `merge/ledger.rs` (320 lines) each define a `Ladder`, `next_tier`, `cancel_request`, `append_error`, a `load` of the Attempt (`Target` and `Attempt`, same join), a cause enum (`Cause`, `Failure`) and an `End` enum, and the transaction that ends an Attempt (`end`, `end_merging`) with the same four Event texts. `driver/lock.rs::probe` and `merge/lock.rs::take_free_driver_lock` are two probes of one lock, one creating the file and one not (the cause of the stray lock files).
- **Where.** `core/src/driver/ledger.rs:167,239`, `core/src/merge/ledger.rs:179,273`.
- **Evidence.** `grep -n "struct Ladder" -r core/src` → both files; `diff` of the two `next_tier` bodies (after renaming `Target`/`Attempt`, `Cause`/`Failure`) differs only in the signature line; `cancel_request` differs only in how it takes its arguments; `grep -c "The Job is queued again at Tier"` → 1 in each file. `liveness.rs` imports the driver's `Ladder`, `recover.rs` the merge's.
- **Impact.** A change to the ladder or to an Event text must be made twice, and W2/W3-style fixes (a new cause) need two edits and two `match`es.
- **Proposed fix.** One `attempt/ledger` module: one `Ladder`, one `end(tx, attempt, End)` where `End` covers merged, cancelled and failed, one probe. Behaviour is pinned by `cli/tests/merge.rs` (34 tests) and `cli/tests/driver.rs`.
- **Size.** Job.
- **Overlap.** none.

### engine-G2 — three A9 kills, two probes, nine flock sites

- **What.** The window kill is written in `driver/worker.rs` (with retry), `watch/liveness.rs::end_dead` and `merge/recover.rs::rest_of_a9`; `flock` is taken at nine places with their own retry-on-`EINTR` loops.
- **Evidence.** `grep -rn "Flock::lock\|\.try_lock()\|FlockArg::" core/src` → `context/lock.rs`, `session/port.rs`, `watch/lock.rs` (×3), `merge/lock.rs` (×4), `driver/lock.rs` (×2), `store/mod.rs`, `doctor/state.rs`, `pr/poll.rs`.
- **Proposed fix.** One `lock` module in `core` with `blocking`, `try`, and `shared`; W3's shared kill. Behaviour pinned by the existing lock tests (`cli/tests/store.rs`, `merge.rs::the_per_repo_lock_…`, `watch.rs::watch_holds_the_lock_…`).
- **Size.** Job.
- **Overlap.** `store-sql` and `commands` areas own two of the nine sites (`store/mod.rs`, `session/port.rs`, `pr/poll.rs`, `context/lock.rs`).

### engine-G3 — driver logs carry no time

- **What.** `watch.log` lines have an RFC 3339 time; `run/<plan>/logs/attempt-<id>.log` lines are `cpd attempt run: A4: waiting for the Worker` with none (`log()` is an `eprintln!`).
- **Where.** `core/src/driver/run.rs:117-119`, against `core/src/watch/log.rs`.
- **Evidence.** `cat ~/.cpd/run/quality/logs/attempt-store-sql-a1.log` prints six lines, none timed, including `reload: new cpd installed, re-executing`; the incident timelines of hs-n79 and hs-n81 had to be rebuilt from `watch.log` and build times.
- **Proposed fix.** Share `watch::log::timestamp` (move it to `core`), prefix each driver line. Test: a line matches the timestamp pattern.
- **Size.** Job (15 minutes).
- **Overlap.** none.

### engine-G4 — the idle and nudge decision is not unit-testable

- **What.** `Driver::wait` is 120 lines: poll panes, record the session file, read the row, handle `stop_request`, judge idleness, nudge, decide `no_submit`. The idle logic (`Some("stop")`, `toolUse`, other `stopReason`) is inline, so it is tested only through a real tmux and a fake pi (`cli/tests/driver.rs`: 19 tests, fakes that sleep up to 120 s); W6 lives exactly there.
- **Where.** `core/src/driver/run.rs:355-434`.
- **Proposed fix.** Extract `fn judge(state, last_stop, turns, nudged: Option<u64>) -> Decision` (pure) and test it with a table; W6's fix becomes a one-line test.
- **Size.** Job.
- **Overlap.** none.

### engine-G5 — `host/pi.rs` holds four concerns in 931 lines

- **What.** `PiDirs` (settings and session directories), the launch argv and environment per role, the package-identity rules, and `session_file`; `driver/run.rs` (582 lines) holds A1–A9 in one `impl`.
- **Where.** `core/src/host/pi.rs`, `core/src/driver/run.rs` (`wc -l`).
- **Proposed fix.** Split `pi.rs` into `dirs`, `launch`, `packages`, `session_file`; no behaviour change.
- **Size.** Job.
- **Overlap.** `pi-pin` (merged) and `manager` edit `host/pi.rs` (`git diff --stat origin/main...origin/cpd/manager/cpd`: 23 lines); do it after `manager` lands.

### engine-G6 — `Report` keeps every failing object for ever

- **What.** `Report.open` and `Report.appended` (`HashMap`s keyed by step, object and code) are never pruned; an object that stopped failing and was archived stays in both.
- **Where.** `core/src/watch/report.rs:113-122`.
- **Evidence.** Bounded by distinct failing objects: 15 `error` Events in 4 519 over the machine's life, and the engine's RSS is flat; so the growth is bytes. Policy hs-d34 says no memory leaks.
- **Proposed fix.** Drop entries of `appended` older than `error_repeat_seconds` during `fail`. Test: after the period the map is empty.
- **Size.** Job (10 minutes).
- **Overlap.** none.

## Already good, keep

- **The tick's SQL**: 5–82 µs a statement, all on indexes except F2; `attempts_active` is a covering index; the catalog is cached (5 ms).
- **Steady-state footprint**: engine 12–14 MB flat, 8–9 fds, drivers 11 MB and 8 fds, driver logs 278 B median, `run/` 11 MB for 20 Plans.
- **The flock design**: kernel-released locks (engine, driver, per-repo); fd 3 made close-on-exec at A1 and kept across `exec`; `Starter` holds the lock from before the insert to after the spawn; E2's probe does not create the file. `lock::remove` after the end commit is guarded by a re-read, so E2 racing a finishing driver does nothing.
- **Compare-and-swap everywhere**: `ledger::end` and `end_merging` swap on the Attempt's and the Job's status; the merge's `update-ref old new` is a git CAS with three retries; E4 re-checks its condition inside the transaction; the cancel safe points (before A3, A8 steps 1 and 7).
- **R** (recovery) is idempotent and covered by tests: `cli/tests/merge.rs` has 34, including death after step 8, and E2's `e2_recovers_a_merge_whose_driver_died_after_the_compare_and_swap`.
- **`Report`**: one `error` per (step, object, code) until it clears, and at most one per `error_repeat_seconds` even if it clears (unit test `the_same_error_is_appended_once_per_repeat_period_even_when_cleared`; the `tmux_failed` storm of 2026-10-15, 8 lines in the log, was fixed by `Line::UnknownView`).
- **Hooks**: process groups killed at the deadline and when `sh` exits (teardown.md G8 is fixed: `exec.rs:wait`), `TMUX`/`PI_*`/`CPD_*` withheld, a per-hook deadline.
- **Concurrent `git worktree add -b`**: 6 in parallel × 20 rounds in a scratch repo (100 files): 120 worktrees, 0 failures, so A2's start of several Attempts at once needs no per-repo lock.
- **Teardown items of `docs/research/teardown.md` now in main**: G3 (the Worker Spec's "stop every process you started" rule, `spec/worker.md`), G8 (above), G12 in the driver (`worker.rs::retry_kill`, three tests).
- **No compiler warnings in the engine area**: the four warnings of `scripts/cargo.sh build --release -p cpd` are in `plan/row.rs` and `tui/src/plan/actions.rs`, none in `watch`, `driver`, `merge`, `attempt`, `ask`, `git`, `host`, `metrics` or `spec`.
- **Small per-call costs**: `pi::session_file` scanning 324 session directories 4.29 ms (N=30, p95 4.70 ms, max 4.94 ms, a miss = the worst case, only until the file exists); `metrics::parse_file` of a median Worker session (139 kB) 0.35 ms, of the largest (1.05 MB) 2.09 ms, of an 8.4 MB Lead session 13.9 ms (N=20 each, release build, via a temporary `core/examples` file, deleted).

## Method

Run from `/home/clayton/projects/cpd.quality--engine-a1` (worktree of Attempt `engine-a1`); nothing was run against the live `~/.cpd`, the live engine or the `-L cpd` tmux server except read-only `ps`, `/proc` and file reads.

- **The copy.** `T=$(mktemp -d /tmp/engine-a1.XXXXXX); sqlite3 ~/.cpd/cpd.db ".backup $T/cpd.db"; cp ~/.cpd/config.toml $T/; cp -r ~/.cpd/contexts $T/`; `printf '\n[tmux]\nsocket = "quality-%s"\n' $$ >> $T/config.toml`. Counts at copy time: 20 Plans, 180 Jobs, 183 Attempts, 4 519 Events, 194 Sessions, 25 PRs, 5 879 txns; DB 26 MB. Commands run on it: `CPD_HOME=$T target/release/cpd context list`, `context show --context building-cpd`, `tiers`, `--version`; `sqlite3 -readonly $T/cpd.db …`; Python `sqlite3` for the timings. `$T/scale.db` was a scratch copy of it (triggers dropped, 9 700 Sessions).
- **Build.** `scripts/cargo.sh build --release -p cpd` (1 m 12 s); a temporary `core/examples/engine_a1_bench.rs` (`scripts/cargo.sh build --release -p cpd-core --example engine_a1_bench`) timed `pi::session_file` and `metrics::parse_file`; a temporary `cli/tests/merge_probe.rs` (copy of `merge.rs` plus two tests) ran `scripts/cargo.sh test -p cpd --test merge_probe probe_a_commit -- --nocapture` and `… probe_r_leaves …`. Both temporary files were deleted before the commit.
- **Live, read-only.** `ps -eo pid,etime,rss,args | grep 'cpd '`; `/proc/589549/status`, `/proc/<pid>/fd`, `/proc/<pid>/stat` (CPU) for the engine and five drivers; a 12-sample loop of `VmRSS`/`VmHWM`/fd/thread counts every 10 s; a 75 s scan of `/proc/589549/task/*/children` every 20 ms for the poll cadence; a 120 s `cutime`/`cstime` delta of the new engine (pid 1557271); `du -sh ~/.cpd/run`; `ls -l ~/.cpd/run/*/logs/*.log`; `grep ~/.cpd/run/watch.log`.
- **Other.** `git ls-remote --heads origin main` ×10; `npm ci --no-audit --no-fund` ×3 in a temp directory holding only `package.json` and `package-lock.json`; a private tmux server (`TMUX_TMPDIR` in a temp dir, socket `quality-eng-a1-<pid>`, 31 panes) for `list-panes -a` ×50 and `list-sessions` ×50; a scratch git repo for 20 rounds of 6 parallel `git worktree add -b`. All of it was removed afterwards (`tmux -L <socket> kill-server`, the temp directories, `$T`).
- **Read, not run.** `core/src/{watch,driver,merge,attempt,ask,git,host,metrics,spec}/` in full except `host/exe.rs`, `host/layout.rs`, `host/pi.rs` beyond launch and `session_file` (pi-pin's); contract §3, §6, §7; commands §12; `docs/research/teardown.md`, `docs/research/gh-interface.md`; Context records hs-d34, hs-d39, hs-d63, hs-n12, hs-n27, hs-n35, hs-n52, hs-n60, hs-n67, hs-n73, hs-n79, hs-n81, hs-d107, hs-d139, hs-d140, hs-d151, hs-d152, hs-d153 (`cpd context show --context building-cpd` on the copy); in-flight branches `origin/cpd/{pi-pin/cpd,manager/cpd,dev-speed/cleanup,cross-repo/plumbing}` by `git diff --stat origin/main...` on the engine area.
- **Not measured.** A hung git or tmux (W4, W8), a reload between nudge and second idle (W6), two concurrent `watch --ensure` (W10): each needs the engine or a Worker flow; all are labelled unverified above. The cost of a 100-PR poll; macOS numbers.
