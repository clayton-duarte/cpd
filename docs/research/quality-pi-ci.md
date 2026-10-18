# Quality research: the pi adapter, the skill, scripts, CI and hooks (area `pi-ci`)

Attempt `pi-ci-a1`, Plan `quality`. Research only: no code, test, script, CI or doc was changed. Base of this worktree: `5ef69b2` (a few commits behind `origin/main` `8d73d96`; the diff of `.github/`, `scripts/` and `.config/` between them is only dev-speed's sccache in `ci.yml` and `cargo.sh`, which this file takes into account).

## Summary

The adapter is in good shape: every contract rule I traced (cursor, restarts, once-per-code errors, idempotent handlers, serialised state reports, plannotator boundary parsing) is implemented and has a test, `KINDS` is pinned to contract §5.2, and `PiApi` is type-compatible with the real pi 1.3.0 except for `registerTool`. The defects are at the edges: an Event kind the adapter does not know costs one error turn per Event and the Event is lost (W1, concrete with the Manager's `session-message`); the state reporter can leave a Session in the wrong state after one refused call (W2, reproduced); the pi tests have a load flake (W3, reproduced 1 in 8 under load); a failed `session show` at start leaves a Lead without Events for the whole pi session (W4); no `pi.exec` call has a timeout (W5). CI is sound but advisory (hs-d74): it cancels the runs of `main` pushes (W7, 2 of the last 20 runs) and `ubuntu-latest` moves to Ubuntu 26 on 2026-10-19 (W8). Measured: `npm run typecheck` 0.50 s, `npm test` median 9.9 s (N=5), the `pi` CI job 32 s, the `rust` CI job median 307 s (N=18) of which `cargo test` is 258–272 s, a cold release build (the cost of an install) 89 s and an incremental one about 18 s. The one thing in this area that is slow is not in `pi/`: it is the Rust test step (area `tests-build`).

## Findings

| Id | Class | What | Size |
|---|---|---|---|
| pi-ci-W1 | Work | An Event kind the adapter does not know is delivered as a turn-triggering error, once per Event, and the Event is lost | Job |
| pi-ci-W2 | Work | `Reporter` dedupes against the last state it tried, not the last it got accepted: one refused call can leave the Session in the wrong state | Job |
| pi-ci-W3 | Work | pi test flake: `Fixture.close` removes the temp dir while a killed fake `cpd` may still write (1 of 8 runs under load) | Job |
| pi-ci-W4 | Work | A failed `session show` at `session_start` is never retried: the Lead gets no Events until `/reload`; permanent follower failures retry forever | Job |
| pi-ci-W5 | Work | No `pi.exec` call has a `timeout` or `signal`, though pi 1.3.0 offers both: a hung `cpd` blocks `session_start` and the tool | Job |
| pi-ci-W6 | Work | Deferred teardown G9: the `cpd-feedback-*` temp dir leaks when pi exits inside `plan decide` (hs-n27) | Job |
| pi-ci-W7 | Work | CI `cancel-in-progress` also cancels `push` runs on `main`: 2 of the last 20 runs, a main commit can end with no CI | Job |
| pi-ci-W8 | Work | `ubuntu-latest` becomes Ubuntu 26 on 2026-10-19 (CI annotation); the rust job's tests drive real `tmux` | Job |
| pi-ci-W9 | Work | `install-cpd.sh` swaps the adapter with `rm -rf` then `mv`, and no test checks that the adapter is installed | Job |
| pi-ci-W10 | Work | The quick check (`pre-commit`) runs no pi test: a Job that breaks `pi/` is merged and caught only at the PR's Gate (by design, R19/R20) | decision |
| pi-ci-F1 | Fast | CI's `rust` job is 307 s median; the Rust test step is 258–272 s of it (cross-area: `tests-build`) | Plan |
| pi-ci-F2 | Fast | sccache's effect on CI is unmeasured: one run so far, cold (build step 12 s → 86 s, 0 hits); re-measure before calling it done (dev-speed) | Job |
| pi-ci-F3 | Fast | `npm test`: 9.9 s median for 66 tests; 30 fixed 150 ms `settle()` sleeps are at least 4.5 s of the 18 s of test time | Job |
| pi-ci-F4 | Fast | An install is a cold release build per worktree: 89 s, 1.4 GB `target/` (dev-speed, hs-q5) | Job |
| pi-ci-G1 | Good | `pi/lib/pi.ts` hand-copies pi's API types; only `registerTool` already differs from pi 1.3.0 | Job |
| pi-ci-G2 | Good | `skill.test.ts` checks flags and a few phrases, not the codes, kinds, numbers and ids the skill states | Job |
| pi-ci-G3 | Good | `commands.md` §15 carries three stale statements (a "follow-up Job 17", two "unverified … the adapter Job checks") | Job |
| pi-ci-G4 | Good | `adapter.ts` is one 504-line closure; `Run.follower` is filled with a cast; 10 `as string` casts after validation | Plan |
| pi-ci-G5 | Good | Six dead-code warnings in every build, and CI does not fail on warnings (cross-area: `engine`/`tui`) | Job |

## Work

### pi-ci-W1: an unknown Event kind is one error turn per Event, and the Event is lost

- **What:** `parseLine` rejects any `kind` not in `KINDS`; `handleLine` delivers that as an `invalid_event` error message with `triggerTurn: true` and moves the cursor past the line. So a cpd newer than the adapter that appends a new kind makes every Lead running the old adapter burn one LLM turn per such Event, and the Event itself is never delivered.
- **Where:** `pi/lib/event.ts:62`, `pi/lib/adapter.ts:239-243`, `pi/lib/delivery.ts:30` (`FOLLOW_UP(true)`); contract in `docs/commands.md` §15.2 ("A line that is not a valid Event (… a known kind) is delivered as an error message and skipped").
- **Evidence:**
  ```
  $ node -e 'import("./pi/lib/event.ts").then(m=>console.log(JSON.stringify(m.parseLine(JSON.stringify({id:5,at:"x",kind:"session-message",context:null,plan:"p",job:null,attempt:null,session:null,text:"t",data:{}})))))'
  {"type":"invalid","reason":"unknown kind \"session-message\"","id":5}
  ```
  `session-message` is the kind the Manager Plan adds (`git diff origin/main...origin/cpd/manager/cpd -- pi/lib/event.ts`: `+ "session-message"` in `KINDS`, migration 0009). `KINDS` equals the Rust `Kind::ALL` today (41 = 41, compared with a script), so the skew exists only between an installed cpd and an already-running adapter.
- **Impact:** after the install that brings `session-message`, every running Lead and Manager session still on the old adapter gets one error turn per message and loses the message itself, until its pi restarts. Cost is tokens and a confused Lead, not data loss in the DB.
- **Proposed fix:** an unknown kind is skipped with the cursor moving past it and one error per kind per run, `triggerTurn: false` (a contract change in §15.2, since the contract says "a known kind"). Test: a line with kind `nope` delivers one non-turn error for two lines.
- **Size:** Job.
- **Overlap:** Manager adds `session-message` to `KINDS` (its branch `cpd/manager/cpd`), which fixes the instance, not the class. pi-pin's "Leads restart on a new pi" (`a4298aa`) and cross-repo's hs-d141 research (`docs/research/pi-reload.md` on `cpd/cross-repo/plumbing`: the adapter reloads pi itself after an install) shrink the window; neither changes how an unknown kind is treated.

### pi-ci-W2: `Reporter` skips a state it already tried, even when the call was refused

- **What:** `pump()` sets `lastKey` before `send` and never clears it when `send` fails. After a refused `session state idle`, a later `want(idle)` (every `agent_settled` and `refresh`) is dropped as "unchanged", so the database keeps the old state (`working`) until a different state is wanted.
- **Where:** `pi/lib/state.ts:103-104`; contract `docs/commands.md` §15.4 ("A refused call … the next change is tried normally").
- **Evidence:** a script that drives the real class (written outside the repo, removed after; run with `node reporter.mts`):
  ```ts
  import { Reporter } from "<worktree>/pi/lib/state.ts";
  const sent: string[] = [], errs: string[] = []; let fail = false;
  const r = new Reporter(async (w) => { sent.push(w.state); if (fail) throw new Error("db busy"); }, (e) => errs.push((e as Error).message));
  r.want({ state: "working" }); await r.settled();
  fail = true;  r.want({ state: "idle" }); await r.settled();   // refused once
  fail = false; r.want({ state: "idle" }); await r.settled();   // the same state wanted again
  console.log(JSON.stringify({ sent, errs }));
  ```
  prints `{"sent":["working","idle"],"errs":["db busy"]}`.
  Only two calls were made; the third `idle` never was. The existing test (`pi/test/state.test.ts:87`) passes because it wants a different state next.
- **Impact:** a transient refusal (`illegal_transition`, a busy store: `store_busy_ms` is 5000) leaves the TUI showing `working` for an idle Lead (or `blocked` for an unblocked one) until the next turn. Unverified: how often a transient refusal happens in practice.
- **Proposed fix:** set `lastKey` only after `send` resolves (or reset it in the `catch`). Test: refuse once, want the same state again, expect a second call.
- **Size:** Job (about 15 min).
- **Overlap:** none found in the Plans in flight (`manager` touches `adapter.ts`, not `state.ts`).

### pi-ci-W3: the pi tests flake under load: `Fixture.close` races a killed fake `cpd`

- **What:** `Fixture.close` fires `session_shutdown` (which `SIGTERM`s the fake follower without waiting) and then `rmSync`s the temp dir. The fake follower's SIGTERM handler writes `killed-<pid>` into that dir, so on a loaded machine the write can land inside the removal.
- **Where:** `pi/test/fake.ts:146-153` (`close`), `pi/test/fake.ts:24` (the handler), `pi/lib/follower.ts:63` (`kill("SIGTERM")`, no wait).
- **Evidence:** `npm test` 15 times: 10 at ambient load (about 12) and 5 earlier: 0 failures. Then 8 runs with 16 `nice -n 10` busy loops added (load 31): 1 failure:
  ```
  not ok 38 - an Event-only decision of a review the Plan has superseded says the Jobs changed
    failureType: 'hookFailed'
    error: "ENOTEMPTY: directory not empty, rmdir '/tmp/pi-test-uExOK4'"
  ```
  (`pi/test/plan-review.test.ts:253`). That the cause is the `killed-<pid>` write is my inference from the code, not shown.
- **Impact:** up to six Workers build at once (hs-d136), so load like this is normal on this machine and the Gate runs `npm test` (`npm run verify`): an intermittent `gate_failed` that has nothing to do with the diff. Same class as hs-n12.
- **Proposed fix:** in `close`, wait for the followers to exit (the fake writes the `killed-` file, so `waitFor` on every pid in `pids()`), or `rmSync(..., { maxRetries: 5, retryDelay: 50 })`. Also bound the timing assertions that are upper bounds, for example `gaps[3]! < 400` in `pi/test/follower.test.ts:41` (unverified that it flakes; it was not seen failing).
- **Size:** Job.
- **Overlap:** hs-n12 (the Rust flake fixed by the hardening Plan) is the same class in another area; the `tests-build` Job covers the Rust tests.

### pi-ci-W4: a failed `session show` at start is never retried; permanent follower failures retry forever

- **What:** `start()` returns after delivering an error when `cpd session show` fails (a busy store, `cpd` briefly missing during an install): no follower is started, so the Lead gets no Events for the rest of the pi session unless the User runs `/reload`. The follower, by contrast, retries every failure forever at up to 30 s: a permanent one (`cursor_ahead`, `session_not_found`, `docs/commands.md` §11.2) costs one `cpd` spawn per 30 s for as long as pi lives (the error is delivered once per code).
- **Where:** `pi/lib/adapter.ts:105-111`; `pi/lib/follower.ts:121-127` (`restartLater` never gives up). Pinned by `pi/test/start.test.ts:109` ("a failing session show is delivered as an error and nothing else starts").
- **Evidence:** by reading the two places and the test; I did not reproduce a failing `session show` against a real `cpd` (not allowed on the live store, and the copy has no pi).
- **Impact:** a Lead that is silently deaf is the worst failure for this adapter (no `job-merged`, no `escalation`; the Worker blocks until `ask_minutes`, 60 min). The error message with a turn tells the Lead, which mitigates it.
- **Proposed fix:** retry the start-up `session show` with the follower's own backoff (1 s up to 30 s) until it works or `session_shutdown`; give up the follower after N consecutive failures of the same permanent code, with one final error. Contract change in §15.2.
- **Size:** Job.
- **Overlap:** hs-n73 / hs-d140 (followers outliving an install, owned by cross-repo `plumbing`: `events follow` exits non-zero on store errors and re-execs itself) make the follower side better; the start-up side is not covered.

### pi-ci-W5: no `pi.exec` call has a timeout or a signal

- **What:** the adapter wraps `pi.exec(command, args)` and drops the third argument, though pi 1.3.0's `ExecOptions` has `signal` and `timeout`. `start()` awaits `session show` (line 107), `session state idle` (line 161) and `plan show`; the tool awaits `plan review`. A `cpd` that hangs (its own lock waits are bounded by `lock_wait_seconds`, 900 s in `cpd config`, and `store_busy_ms` 5000) holds pi's `session_start` for that long. In-flight calls are not aborted at `session_shutdown` either; `stop()` waits at most 2 s and goes on.
- **Where:** `pi/lib/adapter.ts:69`, `pi/lib/pi.ts:4`; pi: `~/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent/dist/core/exec.d.ts:7-12`.
- **Evidence:** `grep -n 'const exec' pi/lib/adapter.ts` → `69:  const exec = (command: string, args: string[]) => pi.exec(command, args);`, and the `ExecOptions` interface (`signal?`, `timeout?`, `cwd?`). Unverified: that a hang ever happens, and whether pi blocks its UI on a pending `session_start` handler.
- **Impact:** low probability, high cost (a frozen start). Cheap to bound.
- **Proposed fix:** `Exec` takes `{ timeout, signal }`; `callCpd` passes a timeout (for example 30 s; `plan review` more) and `abort.signal` of the run. Test: a fake `cpd` that sleeps, `CpdError("spawn_failed"/"timeout")` within the bound.
- **Size:** Job.
- **Overlap:** none.

### pi-ci-W6: G9, the `cpd-feedback-*` temp dir leaks when pi exits inside `plan decide`

- **What:** `decide` writes the feedback to `mkdtempSync(tmpdir()/cpd-feedback-)` and removes it in `finally`; if pi exits inside `await call(...)` (SIGKILL, a crash, or the SIGTERM of a reload) the `finally` never runs.
- **Where:** `pi/lib/adapter.ts:376` (create) and `:403` (remove).
- **Evidence:** `docs/research/teardown.md` row "`/tmp/cpd-feedback-*`" (12 dirs from 2026-10-14) and gap G9. Today: `ls -d /tmp/cpd-feedback-* | wc -l` → `0` (they were cleaned up), so the leak is not currently visible; the code path is unchanged (code read).
- **Impact:** a few bytes of feedback text in `/tmp` per occurrence; no behaviour change.
- **Proposed fix:** as teardown.md G9: write the feedback to `run/<plan>/feedback-<review>.md` (cpd owns it, overwritten on retry), or `plan decide --feedback-file -` for stdin.
- **Size:** Job (15 min, hs-n35's estimate).
- **Overlap:** **hs-n27 and hs-n35** already own it (a follow-up Plan "when cross-repo merges"; cross-repo `plumbing` is still in dispatch). Not a new finding; listed so it is not lost.

### pi-ci-W7: CI cancels `push` runs on `main`

- **What:** `concurrency: group: ci-${{ github.ref }}, cancel-in-progress: true` applies to every ref. Two merges into `main` in a short time cancel the first one's run, which leaves a `main` commit with no completed CI.
- **Where:** `.github/workflows/ci.yml:12-14`.
- **Evidence:** `gh run list --limit 20 --json databaseId,conclusion,event,headBranch,createdAt,updatedAt,name`: two of 20 are `cancelled` and both are `push` on `main` (`39073461292`, 280 s from creation; `39073387444`, 81 s). Both were followed by a newer `main` run, so the first commit's result was lost.
- **Impact:** CI is advisory (hs-d74) so nothing is blocked, but a red `main` commit between two quick merges is invisible. The two cases lose a CI result, not a decision.
- **Proposed fix:** `cancel-in-progress: ${{ github.event_name == 'pull_request' }}` (cancel superseded PR runs, keep every `main` run).
- **Size:** Job (5 min).
- **Overlap:** none (dev-speed changed the same file for sccache only).

### pi-ci-W8: `ubuntu-latest` becomes Ubuntu 26 on 2026-10-19

- **What:** both CI jobs use `runs-on: ubuntu-latest`; GitHub announces the label's migration to Ubuntu 26 on 2026-10-19. The rust job installs `tmux` from apt and runs the `cli/tests` that drive real `tmux` and `git`; a new image can bring a new `tmux` and `git` with it (unverified).
- **Where:** `.github/workflows/ci.yml:18` and `:40`.
- **Evidence:** the annotation on run `39073557666` (`gh run view 39073557666`): `The ubuntu-latest label will migrate to Ubuntu 26 beginning October 19, 2026.` The same annotation lists `actions/checkout@v4` and `actions/setup-node@v4` as Node 20 actions run on Node 24. Unverified: whether any test depends on the `tmux` version of Ubuntu 24.
- **Impact:** a possible red CI on the day of the switch, with no change in the repo.
- **Proposed fix:** run the Rust tests once on `ubuntu-26.04` (a `workflow_dispatch` run on a branch) before the 19th; pin `ubuntu-24.04` if they fail, and move the pin on purpose later. The two Node-20 actions get new majors in the same change.
- **Size:** Job (small).
- **Overlap:** none. (AGENTS.md: upstream defaults win; this is a pre-announced change, so one verification run is the whole cost.)

### pi-ci-W9: `install-cpd.sh` swaps the adapter non-atomically, and nothing tests that it is installed

- **What:** the binary is replaced atomically (`mv -f` of a temp file) but the adapter dir is `rm -rf`'d and then `mv`'d. A pi that loads `~/.local/share/cpd/pi` between the two commands (a new Lead, `/reload`, pi-pin's restart) finds no adapter. Two concurrent installs share the final path. No test covers the happy path of the copy (`test/` excluded, whole dir replaced): `cli/tests/watch.rs:1061` asserts only that a refused install does not create it.
- **Where:** `scripts/install-cpd.sh:50-51` (`rm -rf "$adapter_dir"` then `mv "$adapter_tmp" "$adapter_dir"`); tests `cli/tests/watch.rs:979-1080`.
- **Evidence:** `grep -n 'share/cpd/pi' cli/tests/*.rs` → one line, `cli/tests/watch.rs:1061`. The window is a few ms (unverified that anything ever loads in it).
- **Impact:** very low probability; the failure would be a Lead that starts without an adapter and no error (pi loads nothing).
- **Proposed fix:** `mv` the old dir aside (`mv "$adapter_dir" "$adapter_tmp.old"`) and then the new one in, or `ln -sfn` a versioned dir; add a test that an install writes `package.json` and no `test/`.
- **Size:** Job.
- **Overlap:** hs-d151/hs-d152 (pi-pin, done) added the schema guard to this script and not the swap; hs-d141 (reload of pi after an install, research in `pi-reload.md` on `cpd/cross-repo/plumbing`) concerns what happens after it.

### pi-ci-W10: the quick check runs no pi test

- **What:** `pre-commit` is `npm run typecheck` and `cargo build --workspace`. A Worker that breaks a pi test (or any Rust test) passes its quick check, merges, and the failure shows at the PR's Gate (`pr open`), then costs a fix Job.
- **Where:** `.config/wt.toml:5-7` (protected path, so a Worker cannot change it); `package.json:8` (`npm test`).
- **Evidence:** the hook file; `npm test` costs 9.9 s median (N=5, below), so adding it to `pre-commit` would cost that per submit.
- **Impact:** by design (R19/R20: "per Job, only the quick check"); recorded for the User only. The pi tests are the cheap half of the Gate, so the question is whether 10 s per pi-touching submit is worth a Gate failure fewer.
- **Proposed fix:** none unless the User wants it: it is a one-line change of `.config/wt.toml`, which only the User edits.
- **Size:** decision.
- **Overlap:** R19/R20 (the recorded design).

## Fast

### Measurements taken (this area's required ones)

All on this machine (16 CPUs), while other Workers were building: the load average was 10–14 during the runs below (25 during the cold debug build). Times are wall clock. Binary: built in this worktree. Nothing ran against `~/.cpd`, the live engine or `-L cpd`.

| What | N | Median | Max | Notes |
|---|---|---|---|---|
| `npm run typecheck` (idle: nothing else of mine running) | 5 | 0.50 s | 0.52 s | tsc 7 (native). Run while my own `cargo build --release` was going: 2.45, 4.16, 4.27 s (N=3, median 4.16) |
| `npm test` (66 tests, 7 files) | 5 | 9.94 s | 14.22 s | runs: 11.90, 9.94, 14.22, 9.67, 9.27; 10 more at the same load all passed; under 16 busy loops (load 31): 14–17 s |
| `npm test` sum of the 66 test durations | 1 | 18.2 s | | the files run in parallel, hence 10 s wall; slowest file `plan-review.test.ts` 11.1 s alone |
| `npm ci` (the `pre-start` hook), warm npm cache | 3 | 0.60 s | 0.79 s | `node_modules` is 34 MB |
| `scripts/cargo.sh build --workspace` cold (the first quick check of a worktree) | 1 | 27.3 s | | 100 s user, 773 MB RSS, load 25 |
| `scripts/cargo.sh build --workspace` no-op | 10 | 0.103 s | 0.114 s | |
| the same after `touch core/src/lib.rs` | 3 | 2.58 s | 2.70 s | |
| `scripts/cargo.sh build --release -p cpd` cold (the cost of an install) | 1 | 89.3 s | | `/usr/bin/time -v`: 290 s user, 20 s system, 1.07 GB peak RSS; 12.4 MB binary |
| the same after `touch core/src/lib.rs` | 3 | 18.2 s | 19.0 s | 16.4, 18.2, 19.0 s |
| the same, no-op | 1 | 0.13 s | | |
| `cpd --version` (what the install guard runs) | 5 | 4.0 ms | 4.4 ms | |
| `node -e 0` (what each fake `cpd` in the pi tests starts) | 10 | 23 ms | 25 ms | |
| CI `pi` job (last 20 runs, from `startedAt`/`completedAt`) | 20 | 32 s | 35 s | min 26 s |
| CI `rust` job (same 20 runs; 2 cancelled ones excluded) | 18 | 307 s | 393 s | min 255 s; the failed run is included (272 s) |
| CI run, `createdAt` to `updatedAt` (the task's literal command) | 20 | 311 s | 398 s | includes queue time; min 81 s (a cancelled run) |

Install cost estimate (the script was not run): `install-cpd.sh` = one `build --release --locked` (89 s cold, about 18 s incremental) + `--version` (4 ms) + one `cp` of 12.4 MB + a rewrite of `pi/` (tens of ms), plus, when an engine window exists, `kill-window` and up to 40 × 0.25 s waiting for the lock (`scripts/install-cpd.sh:76-83`).

CI step times of the `rust` job, from the job logs (`gh api repos/{owner}/{repo}/actions/jobs/<id>/logs`, timestamps of the `##[group]Run` lines):

| Run | Build | Test | Note |
|---|---|---|---|
| `39075649526` (a6c5824, Swatinem cache of `target/`) | 15.9 s | 257.9 s | |
| `39085861827` (e8ad70d, same) | 14.9 s | 268.1 s | |
| `39086296275` (9f95ce3, same) | 11.7 s | 258.7 s | `Finished test profile in 31.67s`; the 49 test binaries run one after the other |
| `39086630844` (8d73d96, **sccache**, `cache-targets: false`) | 86.3 s | 272.2 s | `Cache hits 0`, `Cache misses (Rust) 122`, `Non-cacheable calls 85` |

### pi-ci-F1: the `rust` job is 85 % `cargo test`

- **What:** the CI critical path is the `rust` job (median 307 s). Its test step is 258–272 s (3 runs on the old cache, 1 on sccache); `build` is 12–16 s. The `pi` job (32 s) is never the limit. Of the test step, 32 s is compiling the test profile and the rest is the test binaries (the `finished in` lines add up to about 260 s with 8–22 s for the longest ones).
- **Where:** `.github/workflows/ci.yml:36-37`, `cli/tests/` (40 files; 49 `test result` lines in the run's log).
- **Evidence:** the table above; `gh api repos/{owner}/{repo}/actions/jobs/<rust job id of run 39086296275>/logs | grep -c 'finished in'` → 49.
- **Impact:** every PR waits 5 min for a result nobody blocks on (hs-d74); the Gate (`npm run verify`) runs the same `cargo test --workspace` on the machine, where it is the same bottleneck.
- **Proposed fix:** none in this area: split or speed the Rust tests (`tests-build` owns it: per-binary durations, a shared test harness, nextest). CI-side, `cargo test` is already one invocation.
- **Size:** Plan (cross-area).
- **Overlap:** area `tests-build`; dev-speed (sccache, `cache`) targets the build, not the test run.

### pi-ci-F2: sccache's effect on CI is unmeasured

- **What:** dev-speed's change (`cache-targets: false` for Swatinem, `mozilla-actions/sccache-action`) landed with `8d73d96`. The only run on it is cold: the build step went from 12–16 s to 86 s, `Cache hits 0`, and the test step did not move (272 s). Whether warm runs win it back is not yet known. If sccache does not reach a hit rate that saves more than the 70 s the target cache used to give, the net is a regression.
- **Where:** `.github/workflows/ci.yml` on `origin/main` (not in this base: `git diff HEAD origin/main -- .github`).
- **Evidence:** run `39086630844` log, step table above.
- **Impact:** unknown until two or three warm runs exist (main runs after `8d73d96`).
- **Proposed fix:** after three warm runs on `main`, read `sccache --show-stats` of each (the step prints it with `if: always()`) and compare the `rust` job to 307 s. If the hit rate for the test profile is low, put `cache-targets` back to true or cache the test profile too.
- **Size:** Job (read-only measurement first).
- **Overlap:** **dev-speed** (Plan `dev-speed`, done; this is its acceptance measured on CI, which its PR could not show).

### pi-ci-F3: `npm test` waits in fixed sleeps

- **What:** 66 tests take 18.2 s of summed test time; 30 calls of `settle()` (default 150 ms, "wait long enough for something that must NOT happen") are at least 4.5 s of it, and every `cpd` call is a `node` process (23 ms startup each, plus the fixture's 10 ms polling). It is 10 s wall because the files run in parallel; the slowest file is `plan-review.test.ts` (13 settles) at 11.1 s alone.
- **Where:** `pi/test/fake.ts:263` (`settle`), `pi/test/plan-review.test.ts` (13 uses), `state.test.ts` (7), `delivery.test.ts` (4), `follower.test.ts` and `start.test.ts` (3 each).
- **Evidence:** `grep -c 'settle(' pi/test/*.test.ts`; the per-test durations of `node --test --test-reporter=spec` (largest: 1.12 s "recovery: plannotator not answering…", 0.93 s "a replayed or superseded request is skipped").
- **Impact:** low: 10 s of a gate whose Rust part is minutes. The same sleeps are also what makes the flake of W3 likelier under load.
- **Proposed fix:** not worth doing for speed. If W3 is fixed, replace each negative `settle` by waiting for a positive event that follows the one that must not happen (a marker Event handled after it).
- **Size:** Job (only with W3).
- **Overlap:** none.

### pi-ci-F4: an install is a cold release build in every worktree that has none

- **What:** the install script builds `--release --locked` into `$root/target` (or `CARGO_TARGET_DIR`) of the worktree it runs from. A worktree that has only run the quick check (debug) has no release artifacts: 89 s, 290 s CPU, 1.07 GB. The same worktree's `target/` was 1.4 GB after one debug and one release build.
- **Where:** `scripts/install-cpd.sh:12-14`.
- **Evidence:** the table above (cold 89 s; incremental 16–19 s; no-op 0.13 s).
- **Impact:** a Lead's preview install (hs-d116) costs 1.5 min of 16 cores; six Workers building at once make it longer.
- **Proposed fix:** none new: the shared compile cache (sccache in `scripts/cargo.sh`, landed in `8d73d96`) is the fix; re-measure the cold release build with it warm (`sccache` is not installed on this machine, so it could not be measured here).
- **Size:** Job (measurement).
- **Overlap:** **dev-speed** (shared compile cache) and the binaries research hs-q5 (`docs/research/binaries.md`).

## Good

### pi-ci-G1: `pi/lib/pi.ts` hand-copies pi's API types

- **What:** `PiApi`, `Ctx`, `ToolDefinition`, `Message`, `SendOptions` and `Exec` are written by hand ("structural, so tests pass a fake"), and pi is not a dependency of the repo. A pi change is caught by no `tsc` run; only `scripts/pi-check.sh` (manual, run by dotfiles) loads the adapter in a real pi. AGENTS.md: never copy what upstream owns.
- **Where:** `pi/lib/pi.ts:1-40`; `package.json` (no `@earendil-works/pi-coding-agent` in `devDependencies`); `pi/package.json` (`peerDependencies: "*"`).
- **Evidence:** I compiled `ExtensionAPI` of the installed pi 1.3.0 against `PiApi` in `/tmp/pici-a1/tcheck` (outside the repo):
  ```
  export const asMine: PiApi = real;
  check.ts(4,14): error TS2322: Type 'ExtensionAPI' is not assignable to type 'PiApi'.
    Types of property 'registerTool' are incompatible. … Type 'unknown' is not assignable to type 'TSchema'.
  ```
  and with `Omit<PiApi, "registerTool">` the same compile exits 0: every other member (`on`, `events`, `exec`, `sendMessage`, `appendEntry`, `setSessionName`, `getSessionName`) is compatible.
- **Impact:** a future pi that renames or retypes a member passes `npm run typecheck` and `npm test` and fails only in a user's pi. The adapter's `parameters` is a plain JSON-schema object, not a TypeBox `TSchema`: it works if pi's validator accepts plain JSON Schema (unverified at runtime; not run: starting pi is out of scope).
- **Proposed fix:** a `devDependency` on the pi package at the version pi-pin pins (`cpd tiers` reports `pi_version` 1.3.0), `PiApi = ExtensionAPI` for the parts used, so `tsc` is the drift check and `pi.ts` shrinks to the two or three types the tests need.
- **Size:** Job.
- **Overlap:** pi-pin (done) pins the pi version and added `pi-check.sh`; this adds the compile-time half. Check against the pin before adding the dependency.

### pi-ci-G2: the skill test pins flags and phrases, not the facts the skill states

- **What:** `skill.test.ts` proves that each `cpd …` line names a command of `commands.md` and that its flags exist for those words (the union of every synopsis with the same words), that no cpd 1 text remains, links resolve and one Rules sentence exists. It does not check the Event kinds, error codes, exit codes, numbers (13 min, 150 turns, the 30 min timeout, `retry_cap` 2, the 5 s poll) or id patterns the skill states.
- **Where:** `pi/skills/skill.test.ts:85-97` (flags), `pi/skills/cpd-lead/SKILL.md`, `references/spec-files.md`.
- **Evidence:** a one-off script (not committed) checked today's text: every backticked kebab-case word that is not a hook name (`pre-commit`, `pre-start`, `pre-merge`) is a kind of `core/src/events/kind.rs`; every backticked snake_case word is an `ErrorCode` of `core/src/error.rs` except the tool names and fields (`ask_user_question`, `cpd_plan_review`, `forked_from`, `out_of_scope`, `checks_failed`, `changes_requested`, `check_tail`, `timed_out`, `no_submit`, `merge_conflict`, `retry_cap`, `cancel_requested`, `ask_minutes`, which are causes, reasons and config keys). The Job, PR and Plan id patterns match `docs/contract.md:120-122`; the 30 min timeout matches `attempt_minutes = 30` of §8.2 (the live config sets 90, a deviation). I found no contradiction with `commands.md` §15, §16 or §9: the skill is correct today; it is just not guarded.
- **Impact:** the skill is prose the Lead obeys; a rename of a code or kind (the Manager Plan renames the phase `pr` to `deploy`, `fc6c24b`) is caught only by a human.
- **Proposed fix:** two more tests in `skill.test.ts`: kinds named in backticks and in the Event table ∈ `KINDS` (and `LEAD_MESSAGES`), error codes ∈ a list read from `core/src/error.rs`. AGENTS.md: "Prose is not enforcement".
- **Size:** Job.
- **Overlap:** **manager** rewrites `skill.test.ts` (adds `cpd-manager`, `a0f0f39`, `05f6e1f`): do this after it merges. cross-repo `plumbing` edits `SKILL.md` (`pr push`).

### pi-ci-G3: `commands.md` §15 carries stale statements

- **What:** three sentences in §15 are no longer true.
- **Where:** `docs/commands.md:1770` ("The skill `cpd-lead` is rewritten for cpd 2 (follow-up Job 17)": it was); `:1819` ("unverified for every Lead package; the adapter Job checks each": the adapter exists and the `herdr:blocked` signal is what `pi/lib/adapter.ts:152` reads); `:1862` ("Unverified: … the adapter Job tests it").
- **Evidence:** `grep -n 'follow-up Job 17\|unverified for every Lead\|the adapter Job tests it' docs/commands.md`. Not verified here: whether the two "unverified" claims were ever checked (the first needs a real `ask_user_question`, the second two plannotator servers).
- **Impact:** a reader cannot tell what was verified. Cheap.
- **Proposed fix:** rewrite the three as facts with the evidence, or delete them if nobody verified them: say `not verified`.
- **Size:** Job (10 min).
- **Overlap:** the `commands` area Job reads the same file: tell the Lead to avoid two edits. manager edits §15 (turn-start updates).

### pi-ci-G4: `adapter.ts` is one closure of 504 lines

- **What:** one function holds the lifecycle, the follower's hooks, the Event table, the Plan review, the tool and the state: 16 mutable fields in `Run`, `follower: undefined as unknown as Follower` (an illegal state: a Run without a follower, then filled in) and 10 `as string` casts of `ev.data.x` after `NEEDED` has validated them.
- **Where:** `pi/lib/adapter.ts` (`wc -l` 504), `:121` (the cast), `grep -c ' as string' pi/lib/adapter.ts` → 10.
- **Evidence:** the wc and grep; the Manager's branch changes 99 lines of this file (`git diff origin/main...origin/cpd/manager/cpd --stat -- pi/lib/adapter.ts`).
- **Impact:** every adapter Job touches the same file (Manager, W1, W4, W5 all do), so they collide; a handler that reads an unvalidated field fails only at runtime.
- **Proposed fix:** after `manager` merges: move the Plan review (`openReview`, `decide`, `decidedElsewhere`, `recover`, `settle`, the tool) to `review.ts`, type `CpdEvent` per kind so `NEEDED` becomes the type (no `as string`), and build `Run` with the follower in its constructor.
- **Size:** Plan if done with types, Job for the file split alone.
- **Overlap:** **manager** (rewrites the same file; sequence after it).

### pi-ci-G5: six dead-code warnings in every build, and CI does not deny warnings

- **What:** the build prints six warnings (never-read fields, never-used methods and functions); CI and the Gate run `cargo build` / `cargo test` without `-D warnings`, so a new warning is silent.
- **Where:** `core/src/plan/row.rs:42,57,160,174`; `tui/src/plan/actions.rs:30,44`; `.github/workflows/ci.yml`.
- **Evidence:** `scripts/cargo.sh build --release -p cpd`:
  ```
  warning: multiple fields are never read   --> core/src/plan/row.rs:42:9
  warning: multiple fields are never read   --> core/src/plan/row.rs:57:9
  warning: method `to_json` is never used   --> core/src/plan/row.rs:160:12
  warning: function `query` is never used   --> core/src/plan/row.rs:174:15
  warning: field `run_command` is never read --> tui/src/plan/actions.rs:30:5
  warning: method `open_lead` is never used --> tui/src/plan/actions.rs:44:12
  ```
- **Impact:** noise in every Worker's build output; dead code stays.
- **Proposed fix:** delete the dead code (after checking why it exists: it may be for the `tui-visuals` or `manager` Plans), then `RUSTFLAGS="-D warnings"` in the CI `rust` job and in `wt.toml` (a User edit).
- **Size:** Job.
- **Overlap:** cross-area: `engine` (`core/src/plan/`) and `tui`; **tui-visuals**, **tui-dots** and **manager** edit the same files (`plan/actions.rs` is the cockpit's Plan actions).

### Cross-area notes (one line each, not findings of this area)

- `tests-build`: the CI run `39073557666` (a PR `revert-14-revert-13-cpd/pr-poller`) failed on `a_killed_view_respawns_at_once_and_the_engine_stays_dead`: `cli/tests/tui.rs:51:9 timed out waiting for the tree to respawn` (`gh api repos/{owner}/{repo}/actions/jobs/117515959489/logs`); a second, later run of the same code passed, so it looks like a flake (unverified; one occurrence).
- `engine`: `scripts/install-cpd.sh:61-85` kills the engine's tmux window (`kill-window … :watch`) and restarts it with `--ensure`: engine shutdown on SIGHUP is the engine area's to judge (hs-n27 lists no gap for it).

## Already good, keep

- **The adapter implements §15 closely and is tested through real boundaries**: a fake `cpd` on `PATH` (a real child process, real stdout lines, real exits) and a fake pi; 66 tests pass; `KINDS` is pinned to contract §5.2 by `pi/test/event.test.ts:25` and equals the Rust `Kind::ALL` (41 = 41).
- **Boundary parsing**: one `parseLine`, one `callCpd` envelope check, one `parseRespond` for plannotator; no handler re-validates. `NEEDED` is the single list of fields the handlers read.
- **The cursor design** (`cursor.ts`): the greater of `registered_event` and the last `delivered_to` message, with a fork's inherited messages excluded; the error message carries no `id`, so it never moves the cursor.
- **Follower**: argv spawn, stdin ignored, line framing with flush at close, replaced/stopped child guard (`this.child !== child`), backoff reset only by a valid Event, errors once per code. 
- **Serialised state reports** (`Reporter`): one in flight, only the latest wanted state kept, `close` sends `stopped` with a 2 s bound; apart from W2 it is right.
- **Idempotent handlers**: `Facts.open/close` keyed, `opening`/`deciding` sets, `run !== r` checks after every await, so replays and a `/reload` in the middle are harmless.
- **`scripts/cargo.sh`** (resolution order, loud failure, tested by `cli/tests/cargo_script.rs`) and **`scripts/install-cpd.sh`**'s refusal of a build older than the store (before anything is replaced; hs-d151/hs-d152; tested in `cli/tests/watch.rs:1049`). `scripts/pi-check.sh` is clean (private tmux socket, `trap` cleanup, no live state).
- **CI calls the same four commands as the Gate** (`npm run verify`), split into two parallel jobs; `build --workspace --locked` passes, so `Cargo.lock` is in sync (`scripts/cargo.sh build --workspace --locked` → `Finished`). Permissions are `contents: read`; timeouts are set.
- **The skill is consistent with `commands.md`** today (G2's check) and its test already rejects removed cpd 1 text.
- **`npm run typecheck` is 0.5 s** and covers the tests (`tsconfig.json` includes `pi`); `npm test` lists its files explicitly and every `*.test.ts` under `pi/` is in the list (`find pi -name '*.test.ts'`).

## Method

Commands run (all read-only against the live system; nothing touched `~/.cpd`, the live engine or `tmux -L cpd`):

- Reading: every file under `pi/`, `scripts/`, `.github/workflows/ci.yml`, `package.json`, `tsconfig.json`, `.config/wt.toml`; `docs/commands.md` §11.2, §15, §16, §10.6; `docs/research/teardown.md` (G9); `git diff origin/main...origin/cpd/<plan>/<key> --stat` and the diff of `pi/lib/{adapter,delivery,event}.ts` for `manager/cpd`; `--stat` for `pi-pin/cpd`, `cross-repo/plumbing`; `git diff HEAD origin/main -- .github scripts/cargo.sh .config`.
- Context records (from a `sqlite3 .backup` copy, see below): `select kind, text from context_records where id in ('hs-n27','hs-n81','hs-d151','hs-d152','hs-d74','hs-n12','hs-d63','hs-n73','hs-d157','hs-n54','hs-n60','hs-d140','hs-n35')`. `cpd context list` (live, read-only) once, to see which Plans exist.
- Timing, in this worktree: `npm run typecheck` ×3 (with a build running) then ×5; `npm test` ×5 (timed) then ×10 (pass/fail only) at ambient load; 8 more under 16 `nice -n 10` busy loops (`timeout 150`, they ended and were checked gone); `node --test --test-reporter=spec …` for per-test durations and `node --test <file>` per file; `scripts/cargo.sh build --release -p cpd` under `/usr/bin/time -v` (cold), then `touch core/src/lib.rs` + `build --release --locked -p cpd` ×3, then a no-op; `scripts/cargo.sh build --workspace` (cold) then ×10 no-op, then ×3 after `touch core/src/lib.rs`; `npm ci --no-audit --no-fund` ×3 in a scratch directory with only `package.json` and the lockfile; `target/release/cpd --version` ×5; `node -e 0` ×10; `scripts/cargo.sh build --workspace --locked`. `scripts/install-cpd.sh` and `npm run verify` were not run.
- CI, read-only: `gh run list --limit 20 --json databaseId,conclusion,createdAt,updatedAt,name,event,headBranch,status,headSha`; per run `gh run view <id> --json jobs` (job `startedAt`/`completedAt`; per-step times are not in this API for these runs, so the step table is from the job logs: `gh api repos/{owner}/{repo}/actions/jobs/<job id>/logs`); `gh run view 39073557666` (annotations).
- Scripts written outside the repo and removed (W2's is quoted above; G1's is the two imports and `export const asMine: PiApi = real;` with `tsc --noEmit`): a one-line `node -e` for the kinds comparison and for W1, and a `node -e` for the skill's terms (G2).
- **Copy measured on:** `T=$(mktemp -d /tmp/pici-a1/home.XXXX); sqlite3 ~/.cpd/cpd.db ".backup $T/cpd.db"; cp ~/.cpd/config.toml $T/; cp -r ~/.cpd/contexts $T/`, then a `[tmux] socket = "quality-<pid>"` table appended to `$T/config.toml`. Used with `CPD_HOME=$T target/release/cpd config`, `… tiers` and `… --version` only (reads). No tmux server was started, so there was none to kill; `$T`, `/tmp/pici-a1/` and the busy-loop processes were removed or confirmed gone before submit.
- Load while measuring: `/proc/loadavg` 10–14 (25 during the cold debug build, 31 during the stress runs); the numbers are therefore upper bounds for an idle machine and comparable to what a Worker sees (hs-d136: up to six build at once).
- Not done, and why: no real pi was started (out of scope), so W5's hang, W4's start-up failure, W9's window and G1's `registerTool` runtime behaviour are `unverified`; `sccache` is not installed here, so the cache effect (F2, F4) is read from CI logs only.
