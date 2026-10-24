# Panel sync: commit-to-draw latency, before and after

Job `final-latency`, Attempt 1, Plan `sync` (decision hs-d595 part 3). Findings only; no design. Measured 2026-10-24, 11:38 to 12:20 EDT, on `hs` (AMD Ryzen 7 3800X, 8 cores / 16 threads, Linux 6.8), A/B on one machine in one sitting, from throwaway homes and private tmux servers. It replaces the earlier Attempt's file, which held no measurement. Every number says its unit and its sample count `n`; `verified` says how; `unverified` and `guess:` mark the rest. Times in ms unless a number says s. Percentiles are linear interpolation over the n samples.

## Summary

**Worse than BEFORE (first).** No latency in this file is worse than BEFORE: every AFTER median, p95 and max of every surface is lower than its BEFORE value (tables of section 2). What does not hold against the Plan's text:

- **"Within milliseconds" holds for one path only.** A commit that carries an Event (a Lead's `session-state`, a review, a Job queued, the engine's `inbox-*`) is drawn by the tree, plan view and Dispatch column at a median of 106 to 115 ms (p95 118 to 266 ms, the Inbox rows highest; n=20 to 40 each), one 100 ms frame plus the read: about 3 times better than BEFORE (326 to 383 ms) but not milliseconds. A commit that carries no Event (a `pull_requests` row change, which is what a `pr poll` result writes) is drawn by the plan view and the Dispatch column at a median of 16 to 17 ms but with a p95 of 115 to 117 ms (n=40 each). Cause, checked by `strace` and the frame trace (section 7, n=8 commits per kind): not established. In the probe the Event-carrying commits take about one frame (101 to 120 ms) while the Event-less ones take 2 to 20 ms, but a check with the views under `strace` drew within 2 to 13 ms of every commit of both kinds (section 7), so the cause of the frame is open (a hypothesis, guess: the CLI's commit raises a watcher signal slightly before the data is visible, and the real signal then waits out the 100 ms frame; `strace` slows the view and would hide that race).
- **The engine's ticks in a burst: 3 in a sliding second in 2 of 3 AFTER runs, not "at most 2".** In the 100-commit burst the engine started 2 ticks in the burst second in all 3 AFTER runs, and 3 ticks within a sliding 1 s window in 2 of them (tick starts 497 to 498 ms apart, so three fit in 994 ms; section 6). The views nearly held their promise: 10 reads per view in the burst second in all 3 runs x 3 views, and 11 in one sliding second once (the plan view in run 1, two reads 99 ms apart, 1 ms under a frame; the strace timestamps are the first `pread64` of each read).
- **The tree drew nothing in the burst** (10 reads, 0 draws: the commits changed nothing it draws; its trace shows one line in the whole run). Reads are the promise's unit.

**What improved.** The Inbox row: BEFORE median 2 883 ms, p95 3 788 ms (trigger B, n=40) and 2 833 / 3 947 ms (trigger C, n=38); AFTER 115 / 225 ms (n=40) and 113 / 266 ms (n=38). The engine's tick after a foreign commit: BEFORE median 2.5 s (n=118 over B, C, Q), AFTER 64 to 71 ms. The Dispatch column on a Lead's state change: BEFORE 0 of 20 commits within 6 s (never), AFTER median 111 ms (n=20).

### Summary table

Median / p95 in ms, `n` = commits that showed the change (the 20 commits of a run direction, 2 runs, BEFORE and AFTER interleaved). `none in window` = it did not show within the deadline (6 s for A, 12 s for B, C, Q, T). The verdict column holds the Plan's `within milliseconds`, read as: **yes** = p95 at most 20 ms; **one frame** = p95 at most 150 ms (about one 100 ms frame plus a read; guess: that is the best the Plan's own 100 ms frame permits for a commit that waits for a frame); **no** = above that. These thresholds are mine, not the Plan's.

| Trigger | Surface | BEFORE median / p95 ms | AFTER median / p95 ms | Verdict vs `within milliseconds` |
|---|---|---|---|---|
| A session state (working) | tree | 328 / 477 (n=20) | 110 / 138 (n=20) | one frame |
| A session state (working) | plan view | 326 / 479 (n=20) | 107 / 118 (n=20) | one frame |
| A session state (working) | Dispatch column | none in window (0/20) | 111 / 120 (n=20) | one frame |
| A session state (idle) | plan view | 383 / 491 (n=20) | 106 / 118 (n=20) | one frame |
| A session state (idle) | Dispatch column | n/a (already-true 20/20) | 110 / 120 (n=20) | one frame |
| B review open/close | Inbox row (tree) | 2883 / 3788 (n=40) | 115 / 225 (n=40) | median one frame, p95 above |
| C PR mergeable change | plan view | 339 / 500 (n=40) | 17 / 115 (n=40) | one frame |
| C PR mergeable change | Dispatch column | 277 / 534 (n=40) | 16 / 117 (n=40) | one frame |
| C PR mergeable change | Inbox row (tree) | 2833 / 3947 (n=38) | 113 / 266 (n=38) | median one frame, p95 above |
| Q Job queued | tree (ghost count) | 343 / 477 (n=20) | 109 / 120 (n=20) | one frame |
| Q Job queued / cancelled | status bar option | 3108 / 3747 (n=40) | 80 / 187 (n=40) | median one frame, p95 above |
| T tmux title (control) | outer pane title | 14 / 21 (n=40) | 9 / 18 (n=40) | yes |

Rows with n below 20 or an empty cell, and why:

- A idle, Dispatch column, BEFORE: `n/a`, the surface was already showing the idle row before each commit in 20 of 20: the BEFORE Dispatch column never drew the preceding `working` (row above), so there was no change to time. Reason it cannot apply, not a failure to measure.
- A and Q, "Inbox rows": the commit does not change the Inbox; no such cell exists (a surface that never draws these triggers). Likewise B and Q do not change the plan view or the Dispatch column in the probe's store (checked by hand only for the Dispatch column and the tree in a throwaway home: they stayed unchanged; the plan view was not checked: guess).
- C, Inbox row: n=38 of 40 per binary, because the first `conflict` commit of each run has no Inbox row to close (the predicate "count is 0" was already true; the harness does not wait for a predicate that is already true).
- T (the tmux title) is a control: tmux draws it from an option and the product does not take part; both binaries measure the same.

## 1. Method

- **Builds.** BEFORE `4a071cf5` (`git rev-parse b9b5338b^1`, verified; the parent of the merge of PR sync #307). AFTER `21343e7e`, the tip of origin/main when I started (verified: `git log --first-parent origin/main` shows the merges of PR sync `b9b5338b` #307, engine-tick `fea4e885` #310, facts `7cfd48ac` #309 and views `bd179cbd` #315). `cpd --version` of both: `0.13.45-dev` (the builds differ only by path and by `sha1sum`). Release builds, `scripts/cargo.sh build --release -p cpd`, each in its own `git worktree` under `/tmp/sync-probe-final-latency-build` with its own `CARGO_TARGET_DIR`, sccache on (built in about 1 minute each).
- **Merges between BEFORE and AFTER that are not this Plan's**, all in AFTER and not separable from the Plan's effect: `b9cb782c` #308 box widget, `d209150d` #297 Context owner, `01e7db47` #312 (a test deleted), `feacda92` #313 policy scopes, `439a37e8` #314 fake tmux for tests, `d5bf3c42` #317 yellow Job, `21343e7e` #316 plan PR boxes on the box widget. #308, #316 and #317 touch `tui/` drawing code (checked by `git diff --stat`: tree Inbox, plan boxes, `read/shared.rs`, `read/widgets.rs`); #297, #312, #313, #314 do not touch `tui/src` or `engine/src` (same check, not read in full). Their effect on latency: `guess: small` (the numbers below are dominated by waits of 100 ms and seconds, not by drawing, which the frame trace of section 6 puts at tens of microseconds to milliseconds).
- **A cockpit per run**: `mktemp -d /tmp/sync-probe-final-latency-home.XXXX` as `CPD_HOME`, a `config.toml` with its own tmux socket and nothing else (so `[watch]` is at its defaults: E5 `pr poll` every 5 s in both builds), a private server `tmux -L sync-probe-fl-<pid>-<n> -f /dev/null` (the socket name is at most 32 characters: tmux's `tmux.socket` limit), the seed of `panel-sync-latency.md` (Context `ctx`, Plan `pay` in `deploy`, its Lead `lead1` idle, one open PR, one draft Job; Plan `ghost` with 30 draft Jobs for Q) and the real `cpd tui view tree|plan|widgets` and `cpd watch` in it, each under `exec timeout 1500`, 34 / 216 / 33 columns, 50 rows. A stub `gh` answers the GraphQL poll with an empty valid search (the engine really runs `cpd pr poll` and its `gh` child every 5 s in both builds); section 4 repeats B and C with the stub sleeping 5 s. Nothing touched `/home/clayton/.cpd` or the tmux socket `cpd` (asserted in the helper code: the home prefix and the socket prefix).
- **T0** = the `at` of the commit's `txns` row (ms, stamped in the transaction), read back. **T1** = the middle of the `tmux capture-pane -p` call (one thread per surface, every 20 ms) that first shows the expected text. Resolution about +/- 12 ms (from `panel-sync-latency.md` section 1: a capture takes 1.8 ms median; the hit starts 20 ms after the previous miss). T0 is stamped a few ms before the commit lands (CLI commands take 10 to 30 ms in all; SQL writes 6 ms median in the burst), so a value of 0 to 5 ms means "drawn as the commit landed".
- **Interleaving.** Round 1: A, B, C, Q, T each BEFORE then AFTER; round 2: each AFTER then BEFORE; so every trigger ran BEFORE, AFTER, AFTER, BEFORE (A B B A pattern across the two rounds, each pair back to back). 10 commits per direction per run (20 commits), 2 runs per trigger and binary = 40 commits (A: 20 `working` and 20 `idle`, B: 20 open and 20 close, ...). The gaps between commits are random, 1 to 3 s plus the wait for the surfaces.
- **Load and what ran beside.** `/proc/loadavg` (1 min) before each commit: 1.07 to 3.44 over all runs (per run in the table below); the machine also ran other agents' `pi` sessions, `k3s-server`, `decypharr`, `prometheus` and the live cockpit (16 hardware threads, so no CPU was saturated, but not captured per run: unverified beyond `ps` taken once at 11:57). All probe processes ran at nice 19 (`nice -n19` on the probe, inherited by the tmux server and its panes), as in the earlier findings.
- **Seeded store, not live-size**: tens of rows; read cost on live-size data is `panel-sync-cost.md` section 2, not measured here.
- **Not an N-of-1 afternoon**: one machine, one afternoon; n=20 per direction pair per trigger and binary is small for a p95 (the p95 of 20 samples is an interpolation between the 19th and 20th).

| Run (round) | Start | Load 1 min at start | Load 1 min at end | Per-commit load min to max |
|---|---|---|---|---|
| A BEFORE (round 1) | 11:38:26 | 2.50 | 2.59 | 1.79 to 2.59 |
| A AFTER (round 1) | 11:40:20 | 2.55 | 3.25 | 2.31 to 3.44 |
| B BEFORE (round 1) | 11:41:18 | 3.15 | 2.63 | 2.13 to 3.06 |
| B AFTER (round 1) | 11:43:04 | 2.50 | 2.11 | 2.11 to 2.54 |
| C BEFORE (round 1) | 11:43:55 | 1.94 | 1.86 | 1.68 to 2.11 |
| C AFTER (round 1) | 11:45:42 | 1.87 | 1.86 | 1.80 to 2.11 |
| Q BEFORE (round 1) | 11:46:34 | 1.73 | 1.75 | 1.37 to 1.99 |
| Q AFTER (round 1) | 11:48:20 | 1.93 | 1.63 | 1.63 to 2.01 |
| T BEFORE (round 1) | 11:49:11 | 1.58 | 1.60 | 1.58 to 1.88 |
| T AFTER (round 1) | 11:50:05 | 1.67 | 1.70 | 1.67 to 2.04 |
| A AFTER (round 2) | 11:50:55 | 1.56 | 1.65 | 1.47 to 1.71 |
| A BEFORE (round 2) | 11:51:47 | 1.52 | 1.39 | 1.07 to 1.56 |
| B AFTER (round 2) | 11:53:40 | 1.36 | 1.49 | 1.33 to 1.72 |
| B BEFORE (round 2) | 11:54:32 | 1.53 | 1.50 | 1.41 to 1.86 |
| C AFTER (round 2) | 11:56:19 | 1.57 | 1.71 | 1.52 to 1.90 |
| C BEFORE (round 2) | 11:57:13 | 1.66 | 1.55 | 1.27 to 1.68 |
| Q AFTER (round 2) | 11:58:54 | 1.47 | 1.65 | 1.33 to 1.67 |
| Q BEFORE (round 2) | 11:59:48 | 1.60 | 2.44 | 1.14 to 3.34 |
| T AFTER (round 2) | 12:01:35 | 2.29 | 1.97 | 1.79 to 2.29 |
| T BEFORE (round 2) | 12:02:25 | 1.74 | 1.69 | 1.69 to 1.97 |

## 2. Results per trigger

Cells: `seen/n` (commits that showed the change within the deadline / commits that waited for it), then min, median, p95, max in ms, pooled over the two runs of that binary. An empty `0/n` row is a surface that never showed it.

### A: a Lead's session state (`cpd session state working|idle --session lead1`, an Event `session-state`)

| Commit | Surface | Binary | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|---|
| A working | tree | BEFORE | 20/20 | 220 | 328 | 477 | 506 |
| A working | plan | BEFORE | 20/20 | 252 | 326 | 479 | 485 |
| A working | widgets | BEFORE | 0/20 | - | - | - | - |
| A idle | plan | BEFORE | 20/20 | 210 | 383 | 491 | 500 |
| A idle | widgets | BEFORE | 0/0 | - | - | - | - |  <!-- already-true 20 -->
| A working | tree | AFTER | 20/20 | 6 | 110 | 138 | 208 |
| A working | plan | AFTER | 20/20 | 3 | 107 | 118 | 119 |
| A working | widgets | AFTER | 20/20 | 50 | 111 | 120 | 120 |
| A idle | plan | AFTER | 20/20 | 2 | 106 | 118 | 119 |
| A idle | widgets | AFTER | 20/20 | 1 | 110 | 120 | 120 |

The `A idle widgets BEFORE` row: the Dispatch column never drew the preceding `working` commit, so its predicate (`idle`) was already true before each idle commit (20 of 20): nothing to time. AFTER it draws both (median 110 to 111 ms, n=20 each). The tree's idle predicate is not used (the idle dot is also a spinner frame, `U+F0F64`, so an idle tree cannot be told from a spinner frame; same as the earlier findings, which timed the tree on `working` only).

### B: a review opens or closes (`cpd review open|close`; the engine opens or closes an Inbox item at its next tick)

| Commit | Surface | Binary | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|---|
| B open | tree | BEFORE | 20/20 | 1737 | 2556 | 3659 | 3904 |
| B open | engine Event | BEFORE | 20/20 | 1485 | 2165 | 3300 | 3510 |
| B close | tree | BEFORE | 20/20 | 1806 | 3228 | 3788 | 3895 |
| B close | engine Event | BEFORE | 20/20 | 1587 | 2899 | 3532 | 3608 |
| B open | tree | AFTER | 20/20 | 102 | 118 | 213 | 221 |
| B open | engine Event | AFTER | 20/20 | 14 | 80 | 111 | 112 |
| B close | tree | AFTER | 20/20 | 50 | 113 | 314 | 357 |
| B close | engine Event | AFTER | 20/20 | 2 | 65 | 212 | 250 |

### C: a `pull_requests` row changes (SQL `UPDATE pull_requests SET mergeable=..., merge_state=..., changed_at=now`, no Event; what `pr poll` writes)

| Commit | Surface | Binary | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|---|
| C conflict | tree | BEFORE | 18/18 | 1837 | 2343 | 3830 | 4001 |
| C conflict | plan | BEFORE | 20/20 | 233 | 386 | 500 | 507 |
| C conflict | widgets | BEFORE | 20/20 | 14 | 236 | 459 | 533 |
| C conflict | engine Event | BEFORE | 18/20 | 1572 | 2015 | 3549 | 3643 |
| C clean | tree | BEFORE | 20/20 | 1298 | 3047 | 3949 | 4158 |
| C clean | plan | BEFORE | 20/20 | 209 | 321 | 496 | 517 |
| C clean | widgets | BEFORE | 20/20 | 75 | 314 | 561 | 598 |
| C clean | engine Event | BEFORE | 20/20 | 802 | 2655 | 3636 | 3861 |
| C conflict | tree | AFTER | 18/18 | 102 | 113 | 255 | 533 |
| C conflict | plan | AFTER | 20/20 | 5 | 17 | 108 | 110 |
| C conflict | widgets | AFTER | 20/20 | 0 | 11 | 109 | 110 |
| C conflict | engine Event | AFTER | 18/20 | 5 | 87 | 203 | 417 |
| C clean | tree | AFTER | 20/20 | 103 | 113 | 244 | 453 |
| C clean | plan | AFTER | 20/20 | 2 | 17 | 116 | 117 |
| C clean | widgets | AFTER | 20/20 | 3 | 16 | 118 | 119 |
| C clean | engine Event | AFTER | 20/20 | 19 | 56 | 123 | 337 |

The first `C conflict` commit of each run has no Inbox row to close: the tree predicate (count 0) was already true, so the tree rows have n=18 (9 per run), not 20 per run. The engine Event row of C conflict is 18/20 for the same reason in the other direction (the engine had no `inbox-closed` to write for the first commit).

### Q: a Job goes from draft to queued (SQL plus its `job-added` Event) and is cancelled (`cpd job cancel`)

| Commit | Surface | Binary | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|---|
| Q up | tree | BEFORE | 20/20 | 202 | 343 | 477 | 486 |
| Q up | opt | BEFORE | 20/20 | 1917 | 3238 | 3590 | 3686 |
| Q down | opt | BEFORE | 20/20 | 1878 | 2978 | 3784 | 3843 |
| Q up | tree | AFTER | 20/20 | 68 | 109 | 120 | 121 |
| Q up | opt | AFTER | 20/20 | 26 | 71 | 187 | 313 |
| Q down | opt | AFTER | 20/20 | 30 | 97 | 138 | 486 |

Q down changes no tree row (the draft count does not move on a cancel): only the engine-set option.

### T: the tmux title control (`tmux set -p @cpd_title`, drawn by an outer tmux)

| Commit | Surface | Binary | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|---|
| T title | otop | BEFORE | 40/40 | 2 | 14 | 21 | 22 |
| T title | otop | AFTER | 40/40 | 2 | 9 | 18 | 20 |

Both binaries: the tmux pane title appears about 9 to 14 ms after the option is set (n=40 each). It is the harness's own floor, and it is not changed by the Plan.

## 3. Spread between views for the same commit

Largest T1 minus smallest T1 over the surfaces that drew the same commit (n commits, ms). `tree/plan/widgets` of A and C: the tree row there is the Inbox row for C (engine-fed), so C has both a views-only line (`plan/widgets`) and one that includes the Inbox row.

| Commit | Surfaces | Binary | n | min | median | p95 | max |
|---|---|---|---|---|---|---|---|
| A working | tree/plan/widgets | BEFORE | 20/20 | 0 | 58 | 204 | 205 |
| A working | tree/plan/widgets | AFTER | 20/20 | 2 | 15 | 106 | 108 |
| A idle | tree/plan/widgets | AFTER | 20/20 | 1 | 8 | 101 | 109 |
| C conflict | plan/widgets | BEFORE | 20/20 | 18 | 141 | 394 | 420 |
| C clean | plan/widgets | BEFORE | 20/20 | 22 | 113 | 361 | 363 |
| C conflict | plan/widgets | AFTER | 20/20 | 0 | 2 | 99 | 100 |
| C clean | plan/widgets | AFTER | 20/20 | 0 | 1 | 101 | 103 |
| C conflict | tree/plan/widgets | BEFORE | 20/20 | 140 | 2148 | 3416 | 3573 |
| C clean | tree/plan/widgets | BEFORE | 20/20 | 942 | 2887 | 3720 | 3911 |
| C conflict | tree/plan/widgets | AFTER | 20/20 | 0 | 99 | 210 | 517 |
| C clean | tree/plan/widgets | AFTER | 20/20 | 6 | 98 | 231 | 439 |
| Q up | tree/opt | BEFORE | 20/20 | 1587 | 2871 | 3259 | 3476 |
| Q up | tree/opt | AFTER | 20/20 | 11 | 42 | 85 | 209 |

The views fed by the store directly (`plan/widgets` of C, the three of A) differ by a median of 1 to 15 ms and a p95 of 99 to 106 ms AFTER (guess: one view reads at once and the other waits for its frame), against a median of 58 to 141 ms and a p95 of 204 to 394 ms BEFORE. A view fed by the engine (the Inbox row, the option) differs from the store views by the engine's wait: median about 99 ms AFTER, about 2.1 to 2.9 s BEFORE.

## 4. The engine's tick after a foreign commit

Time from a commit by another process (the probe's CLI or SQL write, T0) to the start of the engine's next tick (the change of `locks/watch.lock`'s mtime, polled every 10 ms; `lock.touch()` is called once at the start of each tick: verified in `panel-sync-cost.md` method by `grep` of `engine/src`, not repeated here). Only B, C and Q are valid: their record waits for an engine-fed surface, so the next tick has happened when it is written; A's record is written without waiting for a tick (A BEFORE: 11 of 20 records in round 1 had a next tick), so A is left out (n for the others is below).

| Trigger | Binary | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|
| A | BEFORE | 22/40 | 43 | 1890 | 4573 | 4927 |
| A | AFTER | 38/40 | 12 | 61 | 109 | 270 |
| B | BEFORE | 40/40 | 1473 | 2546 | 3502 | 3600 |
| B | AFTER | 39/40 | 2 | 71 | 120 | 243 |
| C | BEFORE | 38/40 | 794 | 2520 | 3621 | 3856 |
| C | AFTER | 40/40 | 1 | 66 | 162 | 407 |
| Q | BEFORE | 40/40 | 1860 | 3090 | 3730 | 3832 |
| Q | AFTER | 40/40 | 10 | 64 | 172 | 465 |

BEFORE the engine started a tick 1.5 to 3.9 s after a foreign commit (median 2.5 to 3.1 s, n=118); AFTER 1 to 465 ms (median 64 to 71 ms, p95 120 to 172 ms, n=119). "No tick found" cells (the n of 40): the run ended before the next tick.

With a slow `gh` (the stub sleeps 5 s per poll, which models the live cockpit's 4.8 to 7.8 s poll of `panel-sync-latency.md` section 6; 1 run of 20 commits per binary):

| Trigger | Binary | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|
| B (slow gh) | BEFORE | 20/20 | 1482 | 1964 | 3364 | 3423 |
| B (slow gh) | AFTER | 20/20 | 33 | 59 | 95 | 99 |
| C (slow gh) | BEFORE | 19/20 | 372 | 2651 | 3420 | 3424 |
| C (slow gh) | AFTER | 20/20 | 14 | 82 | 116 | 285 |

Inbox rows and engine Events with the slow `gh` (run of 20 commits per binary):

| Commit | Surface | Binary | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|---|
| B open | tree | BEFORE | 10/10 | 2512 | 12293 | 13335 | 13427 |
| B open | engine Event | BEFORE | 10/10 | 2171 | 11836 | 13081 | 13191 |
| B close | tree | BEFORE | 10/10 | 11828 | 12535 | 13740 | 13766 |
| B close | engine Event | BEFORE | 10/10 | 11528 | 12224 | 13435 | 13465 |
| B open | tree | AFTER | 10/10 | 102 | 109 | 168 | 207 |
| B open | engine Event | AFTER | 10/10 | 41 | 63 | 96 | 106 |
| B close | tree | AFTER | 10/10 | 80 | 113 | 214 | 220 |
| B close | engine Event | AFTER | 10/10 | 44 | 73 | 104 | 106 |

| Commit | Surface | Binary | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|---|
| C conflict | tree | BEFORE | 9/9 | 12084 | 13063 | 13630 | 13643 |
| C conflict | plan | BEFORE | 10/10 | 224 | 320 | 484 | 486 |
| C conflict | widgets | BEFORE | 10/10 | 56 | 344 | 528 | 547 |
| C conflict | engine Event | BEFORE | 9/10 | 11754 | 12691 | 13307 | 13355 |
| C clean | tree | BEFORE | 10/10 | 974 | 13261 | 13861 | 13902 |
| C clean | plan | BEFORE | 10/10 | 232 | 325 | 504 | 507 |
| C clean | widgets | BEFORE | 10/10 | 19 | 169 | 563 | 587 |
| C clean | engine Event | BEFORE | 10/10 | 369 | 12978 | 13464 | 13470 |
| C conflict | tree | AFTER | 9/9 | 103 | 116 | 178 | 217 |
| C conflict | plan | AFTER | 10/10 | 2 | 17 | 116 | 121 |
| C conflict | widgets | AFTER | 10/10 | 2 | 8 | 116 | 117 |
| C conflict | engine Event | AFTER | 9/10 | 22 | 84 | 104 | 110 |
| C clean | tree | AFTER | 10/10 | 107 | 116 | 316 | 407 |
| C clean | plan | AFTER | 10/10 | 2 | 7 | 114 | 115 |
| C clean | widgets | AFTER | 10/10 | 9 | 62 | 118 | 119 |
| C clean | engine Event | AFTER | 10/10 | 20 | 84 | 210 | 296 |

BEFORE, the Inbox row waited behind the engine's blocking `pr poll`: median 12.3 s (n=20, B) and 13.1 to 13.3 s (C, n=19 and 10) against a tick that started 1.5 to 3.4 s after the commit (the tick starts, then runs the poll first; the Inbox step E8 comes after it). The stub sleeps 5 s per `gh` call and a poll makes at least two (guess), so this poll is longer than the live cockpit's 4.8 to 7.8 s; the BEFORE result is a model of the live 6.1 s median, not a reproduction. AFTER the Inbox row did not move with the slow `gh`: median 109 to 116 ms (B open, C conflict, C clean; n=10 each), the engine Event 63 to 84 ms (the tick is no longer behind the poll, the Plan's D3). The plan view and the Dispatch column on C were as without the slow `gh` in both binaries.

## 5. The cost of the work: what draws, what reads

Not in scope here beyond the burst below: idle CPU, wakeups and memory of the views and the engine are `panel-sync-result-cost.md`'s (the same Plan's cost job, same commits, merged on this branch's base). I did not repeat them.

## 6. Burst: 100 commits in 1 s

100 SQL commits (each `UPDATE pull_requests SET changed_at=now` plus an Event `session-state` of `lead1`, the state unchanged; in-process writes the way `Store::write` does), 10 ms apart on an absolute schedule (0.99 to 1.02 s), after a 10 s settle and 3 s of quiet, then 6 s of watching. The three views ran under `strace -f -e trace=pread64` and `CPD_TUI_TRACE=1` (tree and plan write the frame trace); the engine had E5 on. A **read** = a burst of at least 15 `pread64` (3 for the Dispatch column) of one thread with gaps of at most 3 ms: the cost findings' detector (verified there: it counted 243 to 244 snapshots for 244 commits in load runs). A **draw** = a trace line. Ticks = `watch.lock` mtime changes, polled every 5 ms. 3 runs per binary, BEFORE and AFTER interleaved (BEFORE, AFTER, AFTER, BEFORE, BEFORE, AFTER).

Cells: in the burst second / over the window from 3 s before to 6 s after / smallest gap between two reads of that view in the burst (ms) / most reads in any sliding 1 s.

| Binary | Run | Writes p50 / max ms | Load | Tree reads (burst s / window / min gap ms / max per 1 s) | Plan reads | Dispatch reads | Tree draws / plan draws (burst s; window) | Engine ticks in burst s / max per sliding 1 s / min gap ms |
|---|---|---|---|---|---|---|---|---|
| BEFORE | r1 | 5.5 / 10.8 | 2.08 | 2 / 3 / 501 / 2 | 1 / 2 / 900 / 2 | 2 / 3 / 605 / 2 | 0; 0 / 1; 2 | 0 / 2 / 41 |
| BEFORE | r2 | 5.7 / 11.5 | 1.29 | 2 / 3 / 599 / 2 | 2 / 3 / 799 / 2 | 2 / 3 / 605 / 2 | 0; 0 / 1; 3 | 0 / 2 / 41 |
| BEFORE | r3 | 5.7 / 11.3 | 1.25 | 2 / 3 / 600 / 2 | 1 / 2 / 801 / 2 | 2 / 3 / 604 / 2 | 0; 0 / 1; 2 | 0 / 2 / 40 |
| AFTER | r1 | 6.0 / 28.7 | 1.72 | 10 / 12 / 100 / 10 | 10 / 12 / 99 / 11 | 10 / 12 / 100 / 10 | 0; 0 / 10; 12 | 2 / 3 / 498 |
| AFTER | r2 | 6.0 / 10.6 | 1.31 | 10 / 11 / 100 / 10 | 10 / 11 / 100 / 10 | 10 / 11 / 100 / 10 | 0; 0 / 10; 11 | 2 / 3 / 497 |
| AFTER | r3 | 6.0 / 9.2 | 2.01 | 10 / 11 / 100 / 10 | 10 / 11 / 100 / 10 | 10 / 11 / 100 / 10 | 0; 0 / 10; 11 | 2 / 2 / 498 |

- **Promise 3, reads per view**: AFTER exactly 10 reads per view in the burst second (n=3 runs x 3 views) and at most 10 in any sliding second except one (plan, run 1: 11, from two reads 99 ms apart); the smallest gap between two reads of one view was 99 ms (plan, runs 1), else 100 ms: **holds for the burst second, marginal by 1 ms in a sliding one**. BEFORE did 1 to 2 reads per view in the burst second (the 500 ms coalesce).
- **Promise 3, engine**: AFTER 2 ticks in the burst second in all 3 runs but **3 in a sliding second** in 2 of 3 (run 3: 2): consecutive tick starts were 497 to 498 ms apart (5 ms polling; the engine's rule is "at least 500 ms after the last one began", so the lock mtime is touched a few ms after the start). So "at most 2 ticks per second" holds for a fixed second and not for a sliding one by 3 ms (3 ticks in 994 ms). BEFORE: 0 ticks in the burst second (the tick was blocked in its `pr poll`; its tick before and after the burst were seconds apart; the `41 ms` minimum is two starts of the engine's own startup ticks, guess).
- **Promise 4, the engine never blocks on a view**: not exercised by this burst; `panel-sync-result-cost.md` section 5 stopped the three views with `kill -STOP` for 20 s and the engine's tick rate and the writers' wall time did not change (8, 8, 7 BEFORE and 44, 44, 44 AFTER starts in 20 s). I did not repeat it: `not measured here`, reason: it is covered by the cost job's measurement of the same builds (the AFTER commit differs: `d5bf3c42` there, `21343e7e` here, both after PR views).

## 7. Cause check: why an Event-carrying commit takes a frame

AFTER only, views under `strace -f -e trace=pread64` and `CPD_TUI_TRACE=1`, 8 commits of each kind 3 s apart: `E` = `cpd session state working|idle` (carries an Event), `P` = `UPDATE pull_requests SET changed_at` (no Event). Per commit, the views' read bursts and the draws (trace lines) as ms after the commit (the writer returned about 10 ms after its stamp, so +10 ms is added):

```text
E | tree reads@9 | draws@11,32,132,233,333,433 | plan reads@9 | draws@13,32,133,233,333,433 | widgets reads@9
P | tree reads@9 | draws@25,125,225,325,425 | plan reads@9 | draws@12,25,125,225,325,425 | widgets reads@10
E | tree reads@9 | draws@12 | plan reads@9 | draws@12 | widgets reads@9
P | tree reads@9 | draws@ | plan reads@9 | draws@12 | widgets reads@9
E | tree reads@9 | draws@13,95,194,294,394,495 | plan reads@10 | draws@13,95,194,295,394,494 | widgets reads@9
P | tree reads@9 | draws@-13,86,187,287,387,486 | plan reads@9 | draws@-13,13,87,187,287,387,487 | widgets reads@9
E | tree reads@9 | draws@-25,12 | plan reads@9 | draws@-24,12 | widgets reads@9
P | tree reads@9 | draws@ | plan reads@9 | draws@13 | widgets reads@9
E | tree reads@9 | draws@12,49,150,249,350,450 | plan reads@9 | draws@11,50,149,250,350,449 | widgets reads@9
P | tree reads@9 | draws@42,142,242,342,441 | plan reads@9 | draws@13,42,142,242,342,441 | widgets reads@9
E | tree reads@10 | draws@13 | plan reads@10 | draws@12 | widgets reads@10
P | tree reads@10 | draws@ | plan reads@10 | draws@12 | widgets reads@10
E | tree reads@10 | draws@13,112,212,313,412 | plan reads@10 | draws@12,112,212,313,413 | widgets reads@10
P | tree reads@10 | draws@8,108,209,308,408,509 | plan reads@10 | draws@9,13,108,208,308,408,509 | widgets reads@10
E | tree reads@9 | draws@-3,12 | plan reads@9 | draws@-2,12 | widgets reads@9
P | tree reads@9 | draws@ | plan reads@9 | draws@12 | widgets reads@9
```

Read: the first column pair is the three views' read bursts (a burst of at least 15 `pread64`; `reads@9` means about 1 ms before the writer returned) and the draws of the tree and plan (trace lines, ms after the writer returned; negative = before it). Under `strace` the views read at the commit and drew 2 to 13 ms later, for both kinds (n=8 each, 16 commits); a second run of the same check with `K=3` and a plan-text watcher (`cause2`, 8 commits) gave the plan text changing at 8 to 13 ms for the Event commits (the later rows with several draws are the spinner's 100 ms frames while the Lead works).

That contradicts the probe: the same Event commits, watched by `tmux capture-pane` without `strace`, change the plan view's text only 100 to 110 ms after the commit. A third check without `strace` and with a single 5 ms observer of the plan row (`seq.py`, 4 commits: working 107 and 102 ms, idle 105 and 101 ms) agrees with the probe. Variants of the probe also agree with it: with the probe's DB-polling thread off, with its read connection off, and with `CPD_TUI_TRACE=1` (12 commits each; most plan values 100 to 120 ms, a few 2 to 20 ms). So the 100 ms is not the harness's database thread, its connection or the trace; what separates the quick runs is `strace` on the view (unverified why). **I report the probe's numbers (they are the commit-to-drawn lag of unmodified views seen by `tmux capture-pane`) and mark the cause open: `unverified`.** A reader should not read the cause check as proof that the frame is not there.

## 8. Not measured

- Idle CPU, wakeups and memory: `panel-sync-result-cost.md` (not repeated here).
- The engine against a stopped view: the cost job's section 5 (not repeated here); a view holding a long read transaction was not tested by either.
- Live-size data, the real `gh` (a stub with an empty search and a `sleep 5` variant stood in for it), a real cockpit's tmux config and the pi chat pane; nothing was measured at nice 0.
- Inbox kinds other than `review-waiting` (B) and `pr-ready` (C); the `A idle` tree (the idle dot is a spinner frame, so the harness cannot tell it).
- The A BEFORE tick (see section 4). Draw cost per frame (the frame trace's `draw_us`) was not tabulated.
- Why a BEFORE tick may start 41 ms after the previous one in the burst (the first ticks after startup, guess).
- n per cell is 20 to 40; p95 and max of 20 samples are coarse.

## 9. Repeat it

The harness is in `/tmp/sync-probe-final-latency` and is removed with this Job; the appendix lists what it holds. To repeat:

```sh
W=/tmp/sync-probe-final-latency; mkdir -p $W/bin $W/out
git worktree add --detach /tmp/sync-probe-final-latency-build/before 4a071cf5; git worktree add --detach /tmp/sync-probe-final-latency-build/after 21343e7e
for v in before after; do (cd /tmp/sync-probe-final-latency-build/$v && CARGO_TARGET_DIR=/tmp/sync-probe-final-latency-build/target-$v timeout 1500 scripts/cargo.sh build --release -p cpd); done
ln -s /tmp/sync-probe-final-latency-build/target-before $W/tgt-before; ln -s /tmp/sync-probe-final-latency-build/target-after $W/tgt-after
pgrep -af sync-probe                                                    # none of an earlier run may be left
cd $W && for t in A B C Q T; do for w in before after; do DEADA=6 DEAD=12 TAG=r1 timeout 400 nice -n19 python3 probe.py $w $t 10; done; done   # ~11 min a round
for w in before after; do nice -n19 timeout 120 python3 burst.py $w r1; done                                # 25 s each
DEAD=22 TAG=r1 timeout 700 nice -n19 python3 probe.py before B 10 5                                          # slow gh (sleeps 5 s)
nice -n19 timeout 150 python3 cause.py                                                                        # section 7
python3 derive.py table A; python3 gen_tables.py; python3 gen_burst.py                                      # the tables of this file
```

## Appendix: new probe code (condensed; the rest is the latency findings' appendix)

Reused unchanged from `panel-sync-latency.md`'s appendix: the seed (extended: Plan `ghost` with draft Jobs `g1` to `g30`, its waiting PR
and Session `lead2`), `cpdw.py`'s one-transaction write (plus a `ROLLBACK` on error), the observer loop (a `tmux capture-pane -p` per
surface every 20 ms, T1 = the middle of the call that first matches) and the `derive.py` percentiles. New: the cockpit launcher with the
socket name `sync-probe-fl-<pid>-<n>` (tmux refuses `tmux.socket` over 32 characters), the triggers below, the tick and Event thread,
the burst and the cause check. Every probe home is `/tmp/sync-probe-final-latency-home.*` (asserted in the write helper), every tmux
call `-L sync-probe-fl-*` (asserted).

```python
# cockpit (cock.py): one throwaway home + private server per run; views and engine `exec timeout 1500 env CPD_HOME=.. PATH=<gh stub>:..`
cfg = 'version = 1\nmachine = "probe"\n[tmux]\nsocket = "%s"\n' % sock          # [watch] left at its defaults: E5 polls every 5 s through the gh stub
tmux(sock, '-f', '/dev/null', 'new-session', '-d', '-s', 'x', '-x', '285', '-y', '50', 'exec timeout 1500 sleep 3600')   # %0 the Lead's pane
# windows: v = tree (%1, 34 cols) | plan (%2, 216) | widgets (%3, 33); e = `cpd watch`;  strace variant: `timeout 1500 strace -f -ttt -e trace=pread64 -o F env ..`
# gh stub: `[ -n "$GH_SLEEP" ] && sleep $GH_SLEEP` then an empty valid GraphQL search (a slow poll is GH_SLEEP=5)
# triggers (probe.py gen(i)); D = spinner frames U+F0F62..U+F0F68, IDLEW = U+F0B94 (the Dispatch row's idle dot), LINK = U+F11AD
A working: cpd session state working --session lead1   tree: pay's glyph != U+F0F64 | plan '#1 open\s+[D]\s+LINK' | widgets '\s+open\s+[D]\s'
A idle:    cpd session state idle  --session lead1     plan '#1 open\s+LINK' | widgets '\s+open\s+IDLEW\s'
B open:    cpd review open --session lead1 --pn pn-<6 hex> --kind plan --subject x --port 20001    tree: Inbox header count == 1
B close:   cpd review close --session lead1 --pn <same>                                           tree: Inbox header count == 0
           count = re.search(r'Inbox\S*?(\d+)[^\d\n]*$', capture, re.M)    # the header is `Inbox───0` BEFORE and `\ue0b6Inbox\ue0b4──\ue0b61\ue0b4` AFTER
C conflict / clean: UPDATE pull_requests SET mergeable='conflicting', merge_state='dirty' | 'mergeable','clean', changed_at=now WHERE id=1
           plan '#1 conflict' | '#1 open';  widgets 'open.*conflict' | no 'conflict';  tree Inbox 0 | 1
Q up:      UPDATE jobs SET status='queued', tier='S3', goal='g', context='c', out_of_scope='[]', fingerprint=<64 x 0>, touches='["a"]',
           acceptance='["a"]' WHERE plan='ghost' AND key='g<30-i>';  INSERT events (kind 'job-added', plan 'ghost')
           tree: 'ghost\s+<29-i>⋅' (the draft count) | option @cpd_workers ' · 1 queued'
Q down:    cpd job cancel g<30-i> --plan ghost --reason p      option ' · 0 queued'
T title:   tmux -L <inner> set -p -t %0 @cpd_title t<i>        outer server attached to the inner: top border line 'T=t<i>' (pane-border-format 'T=#{@cpd_title}')
# per commit: T0 = the `at` of its txn; `already` = surfaces whose predicate was true before the commit (not waited for);
# deadline 6 s (A) or 12 s (B, C, Q, T; 22 s with the slow gh); record {surface: T1 - T0 in ms, engine_event_ms, next_tick_ms, loadavg}
def dbthread():   # 10 ms loop: tick start = change of os.stat(home/locks/watch.lock).st_mtime_ns; engine Event = new inbox-opened|closed row of `events`
    m = os.stat(lock).st_mtime_ns;  ticks.append(time.time()) if m != last else None
    for id, at, kind in db.execute("select id, at, kind from events where id > ? and kind in ('inbox-opened','inbox-closed')", (last,)): evs.append((ms(at), kind))
# next_tick_ms = first tick start after T0, read when the commit's record is written (so only valid where the record waits for the engine: B, C, Q)
# burst (burst.py): 100 x write(UPDATE pull_requests SET changed_at=now; INSERT events 'session-state' lead1) at t0 + i*10 ms, views under strace + CPD_TUI_TRACE=1
#   snapshot = burst of >= 15 pread64 (widgets >= 3) of one thread, gaps <= 3 ms;  draws = trace lines of tree.log / plan.log with their RFC 3339 time
#   ticks = watch.lock mtime changes polled every 5 ms;  burst second = [t0, t0 + 1 s);  windows from t0 - 3 s to the end + 6 s
# cause check (cause.py): 8 Event commits (cpd session state) and 8 `UPDATE pull_requests SET changed_at` commits, 3 s apart, AFTER only:
#   prints, per commit, the views' snapshot bursts and draws as ms after the commit (+10 ms: the writer returned about then)
```

Run order (`run_all.sh`, `run_extra.sh`, `run_extra2.sh`, each `nice -n19`, `timeout`-bounded): rounds 1 and 2, each `A B C Q T` with the
two binaries back to back (round 1 BEFORE first, round 2 AFTER first), `N = 10` per direction (20 commits per run, 2 runs per
trigger and binary); then 3 bursts per binary (BEFORE, AFTER, AFTER, BEFORE, BEFORE, AFTER); then B and C with the slow gh (1 run each, both
binaries); then the cause check.

Cleanup done: no `sync-probe` process of this Job is left (`pgrep -af sync-probe` shows none of mine), every private tmux socket `sync-probe-fl-*` answers `tmux -L <socket> ls` with an error, and `/tmp/sync-probe-final-latency`, `/tmp/sync-probe-final-latency-build` (with its two `git worktree`s) and every `/tmp/sync-probe-final-latency-home.*` are removed.
