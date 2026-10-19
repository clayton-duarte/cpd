# Who held the per-repo lock for "59 minutes"

Job `lock-hold` of Plan `cross-repo`. Read-only; this file changes no code and no contract.

## Answer first

- **The driver of `main-sync-4-a1` did not hold the repo lock for 59 minutes. It waited for it.** Its hold was about one second (22:12:12 → 22:12:13). The "59 minutes" is the driver *process's age* (started 21:09:57), and `fuser` lists every process that has the lock file *open*, waiters included. It cannot show who holds the `flock`.
- **The holder was a chain of `pr open` Gates**, back to back: L3 of `no-repo-lock.md` §1.1 (`pr open` takes the lock, runs the whole Gate, pushes, then drops it: `core/src/pr/open.rs:34-45`). The last link, `cpd pr open --pr cpd` of Plan pulse (pid 2457247), held it from its Gate's start (≤ 22:01:54) to ≈ 22:12:12. The lock was busy with Gates from at least 21:16 to 22:12 (56 min).
- Hypotheses: **(a) refuted** (code, strace), **(b) refuted** (A8's own steps took ≈ 1 s), **(c) refuted** for R and E2; the cause is a fourth one: **the Gate runs under the lock** (a design point, not a bug of one process).
- **No separate fix Job.** PR nolock (hs-d243) deletes the lock and moves the Gate to a throwaway worktree (`no-repo-lock.md` §2.5). Nothing nolock keeps holds a lock, a flock or a long wait across processes (§3). No change is needed in nl-merge, nl-pr or nl-clean; three optional notes in §3.3.

## 1. When the lock was taken and released

All times local (EDT, UTC−4). The Ledger gives UTC; subtract 4 h.

### 1.1 The driver

| Time | Fact | Evidence |
|---|---|---|
| 21:09:57–21:09:59 | driver pid 902044 started | `ps` start time at 22:01:40 (pr-links Lead's session, 02:01:40Z); `~/.cpd/run/watch.log` 01:09:59Z "E3: started main-sync-4-a1 … driver pid 902044" |
| 21:09–21:59 | A2–A5 and the reload cycles in the log. **No repo lock** | `core/src/driver/run.rs`: `reload_if_installed` is called only in A4 for a `running` Attempt (`:380`) and at the top of `merge` (`:507`), both before `RepoLock::wait` (`merge/mod.rs:71`). `RepoLock` is not taken anywhere else in the driver |
| 21:59:17 | Worker's `attempt submit`: `running → merging` (Event 13106) | `cpd events list --plan dev-speed` |
| ≤ 22:01:39 | driver is in `RepoLock::wait` (blocking `flock(LOCK_EX)`, no limit): `fuser` on `repo-1d7f…6.lock` lists 902044 | pr-links Lead's session, 02:01:39Z. The driver opens that file only in `RepoLock::wait` |
| 22:09:01 | `fuser` lists 902044 and 2457247 (`cpd pr open --pr cpd`, started 21:51:47); `ps etime` of 902044 = 59:02 | same session, 02:09:01Z. 59:02 = 22:09:01 − 21:09:57: the process age |
| 22:12:11.3 | pulse's Gate log is written (`fs::write` at the Gate's end, `git/hooks/mod.rs:143`); its last line is nextest's `Summary [617.041s]` | `stat ~/.cpd/run/pulse/logs/pr-cpd.pre-merge.log` |
| ≈ 22:12:12 | the driver gets the lock and steps 1–8 run | reflog of `cpd/dev-speed/migrations`: `cpd: merge dev-speed/main-sync-4-a1` at 22:12:12 (the step-8 CAS) |
| 22:12:13.300 | step-10 transaction: Attempt `ended`, Job merged (Event 13171); lock dropped | Events |
| 22:12:15.2 | pulse's `pr open` finishes `gh pr create` (#73) (Event 13175) | Events; pulse Lead's session 02:12:15.237Z |

So the driver's lock was **taken at ≈ 22:12:12 and released at ≈ 22:12:13**, about 1–2 s. Before that it waited from between 21:59:17 and 22:01:39 until 22:12:12: **10.5 to 13 minutes**, blocked in the kernel with no timeout, no log line, no cancel check and no reload (see §1.3).

### 1.2 Who held it while the driver waited

`fuser` shows two openers, the driver and 2457247. A `flock` has one holder, so one was the holder and the other the waiter. 2457247 held it, because:

- Its Gate ran (`pr open` runs the Gate only after `lock()` returned, `open.rs:34-43`), and the Gate lasted ≥ 617 s (nextest's own summary), so it began ≤ 22:01:54 and ended 22:12:11. For those ≥ 10 minutes the lock was 2457247's, not the driver's. The driver cannot have held it at 22:09:01.
- The driver's CAS comes 0.7 s after the Gate's log, and the driver's step 10 is 2 s before `pr open` finishes: the order of a waiter that got the lock when the holder dropped it after its `push -u`.
- The driver's A8 steps (R, `status`, rebase, CAS, push, `read-tree`, transaction) took ≈ 1 s: CAS 22:12:12, transaction 22:12:13.3. A 13-minute slow step is not in the data.

The chain of holders before it, each a `pr open` Gate under the lock. Starts and ends are those of the callers (Lead sessions, timestamps); "Gate end" is the log's mtime (the log is written once at the end).

| Lock busy until | Holder | Evidence |
|---|---|---|
| ≥ 21:16 → 21:36:18 | Plans sqlfast, register (manager) | sqlfast `pr open` 21:09:58 → ok 21:22:04 (Gate end 21:22:00); register 21:10:03 → opened 21:36:25. reloadfix's first call waited 21:16:00 → `lock_timeout` 21:31:00 |
| → 21:36:34 | pulse `pr open` #1: Gate failed (exit 64: stale `node_modules`, typecheck) | pulse Lead: started 21:28:19, `gate_failed` 21:36:34 |
| → 21:47:18 | reloadfix (cross-repo) | started 21:31:09 (after its timeout), Gate end 21:47:18, opened 21:47:22 |
| → 21:48:21 | patterns (manager) #1: Gate failed | log birth 21:48:21 |
| → 21:58:52 | patterns #2: Gate failed (flaky test) | started 21:48:36, log mtime 21:58:52 |
| → 22:12:11 | **pulse `pr open` #3**, started 21:51:48, Gate ≥ 617 s | above |
| → 22:12:13 | **the driver, A8** | above |
| → 22:12:15 / 22:13:08 | glyphs Gate fails (log birth 22:12:15.27); tests Gate fails (log mtime 22:13:08) | those two had waited since 22:08:40 and 22:09:04 |

Waiters that gave up after the full 900 s (`lock_wait_seconds`, `core/src/config/file.rs:246`): reloadfix 21:16 → 21:31; tests 21:31 → 21:46 and 21:46 → 22:01; pulse #2 21:36:45 → 21:51:45; glyphs 21:53 → 22:08. Each is a `lock_timeout`, as the Manager saw.

That is the whole picture: **the Gate is 10–13 minutes under load ~100–150** (nextest alone 617 s), the lock is per repo, there were 6 PR opens queued in one repo, and the queue wait limit (900 s) is shorter than one Gate behind another. Not one process held it for an hour: the lock was never free for long, so every observer saw it busy.

### 1.3 The three hypotheses

- **(a) A re-exec kept the lock's fd. Refuted.**
  - `RepoLock` opens the file with `OpenOptions` (`merge/lock.rs:116-125`): `O_CLOEXEC`. `strace -e openat cpd plan list` shows `store.lock … O_CLOEXEC` for the same std call (run).
  - The only fd that `exec` keeps is fd 3, the *driver* lock `attempt-<plan>--<attempt>.lock`, on purpose (`driver/lock.rs:143`, `run.rs:268`) and set close-on-exec again in the new process (`driver/lock.rs:197`).
  - The reloads happen in A4 and before `RepoLock::wait`, never while the repo lock is held (§1.1). The `fuser` listing of 902044 is the waiter's open fd.
- **(b) A8 itself was slow. Refuted.** CAS 22:12:12, transaction 22:12:13.3. No `push_failed`, `store_busy` or `lead_worktree_stale` Event for the Attempt (Events 13106–13180).
- **(c) Something else took it outside A8. Refuted for R and E2.** R from a driver only runs under the lock the driver already has (`merge/mod.rs:71-72`); E2 uses `try_take` and skips a held lock (`watch/liveness.rs:160`). The holders outside A8 were `pr open` (L3), as hypothesised for the repo in general, not R.

What the driver's log cannot show: the log has no timestamps, and "A8: merging" is written *before* `RepoLock::wait`, so it cannot tell a wait from a merge. That is why the wait looked like a hold.

### 1.4 What is wrong with the lock, apart from the misreading

1. **A Gate under a per-repo lock is a queue of 10–13 minutes per PR open** while the wait limit is 15. Two Gates ahead of you is a `lock_timeout`.
2. **Unfair**: `wait_for` polls every 100 ms (`merge/lock.rs:18`), so whoever polls right after a release wins; the driver blocks in the kernel and got it first. The Lead of pulse (`hs-n169` in its own words, "lock starvation") re-ran `pr open` after each timeout.
3. **Unbounded and silent at A8**: `RepoLock::wait` has no limit (§7.4), no log line, no cancel check, no reload point. A stopped or cancelled Attempt cannot act while it waits.
4. **Diagnosis**: `fuser` and `lsof` list openers. The holder is shown by `lslocks` or `/proc/locks` (a waiter shows as `->`; `lslocks` exists on this machine). Two Leads and the Manager drew the wrong conclusion from `fuser`.

## 2. The fix

**None as a Job.** The cause is §1.4 item 1 (the Gate under the lock), and nolock removes it: `RepoLock` is deleted; the Gate runs in a throwaway worktree with no lock (`no-repo-lock.md` §2.5, §2.10, Jobs nl-pr and nl-clean).

If nolock is delayed, the only mitigation without code is a User decision in `~/.cpd/config.toml`: `timeouts.lock_wait_seconds` above `gate_minutes` × 60 × the number of queued PRs. Not proposed: it makes the queue longer, not shorter.

No Job for the misreading: `cpd` has no command that names a lock holder, and with the lock deleted none is needed (YAGNI). The note for whoever reads `fuser` again is item 4 above.

## 3. Does PR nolock's design leave a long hold?

Read: `docs/research/no-repo-lock.md` at the base of this branch (§ numbers below are its own). Searched the code on this branch for every `flock`/`Flock`/`try_lock` (`core/src`): `merge/lock.rs` (repo lock, deleted by nolock and the driver lock), `driver/lock.rs` (driver lock), `pr/poll.rs` (`pr-poll.lock`), the store's `store.lock`.

### 3.1 Each step it keeps

| Step | Holds / waits on | Long? | Across a re-exec? |
|---|---|---|---|
| Gate worktree W (§2.5) | A directory and a `git worktree --lock` flag file (`.git/worktrees/<id>/locked`), not a `flock`. Nobody waits on W: two Gates get two Ws. Leftovers are swept by pid | No | A re-exec keeps the pid, so the sweep (`kill(pid,0)`) still sees it alive: correct. The Gate's CLI commands (`pr open|push|rebase`) do not re-exec anyway (only the driver, the engine's `session run` and `events follow` do: `grep exec core/src`) |
| Rule S (§2.3) | Git's `index.lock` for one `read-tree`; waits ≤ 20 rounds of 100–500 ms (≤ 10 s), then stops with no Event | ≤ 10 s, bounded | No |
| Guard U (§2.6 step 5) | Nothing is held. `pr rebase` polls the store ≤ 60 s for a live driver's step 10, only for an Attempt already past its CAS. That span is the push + `read-tree` + the transaction (§2.2): seconds | ≤ 60 s, bounded, ends in `cas_lost` (exit 3, "rerun") | No: the wait is in a CLI command, the driver it waits for is not blocked by it |
| CAS retries (§2.2, §2.6 step 6) | A ref's lock for ≤ 10 s per git call (`core.filesRefLockTimeout=10000`, G2); `CAS_RETRIES` 3 | ≤ 4 × 10 s in the worst case (a stale `.lock`), bounded | No |
| Clean-up (§2.7) | Reads the Ledger and probes each driver lock with a non-blocking `flock` (`driver/lock.rs:104`); it runs inside `pr-poll.lock` as today | No new wait. The `pr-poll.lock` it is under is held for the whole poll, ≤ 60 s for a waiter (`pr/poll.rs:33`); unchanged by nolock | No |
| The driver lock (fd 3) | Held for the driver's life, kept across `exec` on purpose (`driver/lock.rs:143`) | Intended: it is the proof the driver lives (§3.4) | Yes, deliberately; nolock does not touch it |
| E2's R (§2.8) | The dead driver's lock, taken by `try`, for R's duration | R's git calls and push | No (new process per tick, close-on-exec) |

Nothing it keeps is a file lock or a flock between PRs. Nothing waits on another PR. The only waits are bounded (≤ 10 s, ≤ 60 s) and sit in one PR's own commands.

Nolock also removes the cause directly (§1.4): the Gate (L3–L5) no longer holds anything, and A8 has no unbounded wait (`RepoLock::wait` is gone, §2.2).

### 3.2 What the data says about risk 1 of §4

The queue above is also *serialisation*: 6 PR opens in 70 minutes, each Gate 10–13 min at load 100+. With no lock, they run at once, each in a cold worktree (`npm ci`, a cold `target/`; §2.5 "Cost"). I infer (not measured) that on a CPU-bound machine k concurrent Gates take about k times as long each, and `gate_minutes` is 30 (`core/src/config/file.rs:245`): a timeout is a `gate_error`, not a `lock_timeout`, and the Lead re-runs it. The delay moves from a visible queue to a Gate that can time out. Risk 1's measurement should therefore be taken at the machine's usual load with at least 3 Gates at once, and `gate_minutes` is the number to watch.

### 3.3 What must change in the queued Jobs

**Nothing is required** in `nl-merge`, `nl-pr` or `nl-clean` to avoid a long hold. Optional notes, each the Job Worker's call:

- **nl-pr:** record in the report the wall time of 3 concurrent Gates under the machine's real load (§3.2), not one Gate. Two Gates of one PR share the log `run/<plan>/logs/pr-<key>.pre-merge.log`, written once at the end with `fs::write` (`git/hooks/mod.rs:143`): the second overwrites the first, and `details.log` of the first points at the second's text. Cheap to avoid if W's pid is in the name; the design (§2.9) allows two Gates of one PR.
- **nl-merge:** rule S gives up after 20 busy rounds with no Event (§2.3). A *stale* `index.lock` (a killed `git`, e.g. one of the stall kills in this Attempt's own log) would leave the Lead worktree behind with nobody told. If S sees `index.lock` older than a minute when it gives up, it can report `lead_worktree_stale` with the path, and not delete it. Nothing here holds a lock; this is only a silent stuck state.
- **nl-git (not in the question):** `core.filesRefLockTimeout=10000` turns a stale ref `.lock` from a 100 ms failure into a 10 s wait on every git call that touches the ref, then the same failure. Bounded; the error text should name the `.lock` file. A killed `update-ref`/`fetch` leaves one.

## 4. Unverified

- Which process held the lock at each second of 21:59–22:12 is inferred from `fuser`'s two pids, the Gate's duration and the order of the timestamps. I could not read `/proc/locks` then; it is not kept.
- Why the driver won the lock over the polling `pr open`s is an inference (a blocked `flock` is woken at release; a poller sleeps 100 ms).
- The start of each Gate in §1.2 is "after the previous end", from the Leads' timestamps; only the ends are file times.
- The load figure is from the Leads' `uptime` (93–118 at 21:31–22:09).
- The code read is this branch's (`ae6633a`); the running cpd on 21:09–22:12 was the one installed then. The Gate-under-lock code (`open.rs:34-45`) is older than both; I did not diff the installed binary.

## 5. Method

Ledger (`cpd attempt show main-sync-4-a1 --plan dev-speed`, `cpd events list --plan dev-speed|pulse|…`), `~/.cpd/run/watch.log`, the driver log, hook-log birth and mtime (`stat`), the reflog of `cpd/dev-speed/migrations`, the Lead sessions' JSONL (`~/.pi/agent/sessions`, timestamps of `cpd pr open` calls and results, and the `fuser` output), the code on this branch. One `strace -e openat cpd plan list` to confirm `O_CLOEXEC` on std's opens. Nothing else was run against the User's processes; no temporary file outside the worktree except `/tmp/lh-*`.
