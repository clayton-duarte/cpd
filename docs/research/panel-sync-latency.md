# Panel sync: commit-to-draw latency per panel

Job `lag-report`, Attempt 1, Plan `sync` (decision hs-d595 part 3). Findings only; no design. Taken 2026-10-23 on `hs`
(16 cores). The raw data are the runs of the timed-out Attempt `lag-a1` (17:23 to 18:47 EDT; harness and `out/*.jsonl`
in `/tmp/lag-a1-scratch`), re-derived here by a script (Appendix, `derive.py`), plus one fresh run of triggers A and B
and of the tmux probe T (18:49 to 19:10 EDT, section 2.3). Every number says its unit and its sample count `n`.
`verified` says how; `unverified` and `guess:` mark the rest. Times in ms unless a number says s.

## Summary

- A change in the store is drawn by the plan view in 209 to 519 ms (median 332 ms, n=120 over 3 runs) and by the tree
  in 219 to 534 ms (median 350 ms, n=60): about 0.1 s of poll wait, 2 x 100 ms of frame waits, 2 to 4 ms of read. The
  handoff's "within about 0.7 s" holds for them (max 534 ms).
- The widgets view (the Dispatch column) does **not** draw a Lead's state change at all: 0 of 70 in 6 s. Its `KINDS`
  list has no `session-state` (read in source, and reproduced by hand, 1 run); it draws the change only when a listed
  Event or a `pull_requests` change makes it re-read. For a `pull_requests` change it is the fastest view (32 to 513 ms,
  n=70).
- The Inbox row and the engine-set tmux option follow the engine, not the store: the engine's Inbox step lands 11 to
  23 ms after its tick starts (n=148) and the option 11 to 31 ms (n=80); ticks start every 5.000 s (n=1 046 intervals,
  12 runs, max 5 018 ms; E5 off). Cause to tree row: median 2.6 s, max 5.3 s (n=148). The view adds a median 338 ms
  (n=80) after the engine's write.
- On the live cockpit the engine's `cpd pr poll` child takes 4.8 to 7.8 s of wall time (n=26) and E8 comes after it:
  polls start 9.9 to 12.8 s apart (n=24), so E8 runs in two bursts per cycle and the gap to the next one is up to
  12.8 s (derived, n=26). That reproduces `panel-sync-cost.md`'s median 6.1 s and p95 12.6 s for the Inbox lag.
- tmux shows an option within 1 to 22 ms of its being set (n=26, 40 and 40, section 5): not a source of seconds.
- The spread between two views of the same commit: tree against plan up to 213 ms; plan against widgets up to 412 ms;
  Inbox row against plan median 2.4 to 3.4 s, max 4.9 s (PR changes).
- A fresh run of A (n=40) agrees with the raw data within 28 ms of median; the fresh B within 472 ms (a uniform 0 to
  5 s wait, n=20 per direction).

## 1. Method and tools

Probe (design in the Appendix; harness lost with `/tmp` after this Job, so the Appendix holds it):

- A **throwaway** `CPD_HOME` (`/tmp/lag-a1-home.<x>`, config `version = 1`, `machine = "probe"`, `[tmux] socket` the
  private one, `watch.pr_poll_seconds = 4294967295` so E5 never runs, a `gh` stub on `PATH` that exits 0) and a
  **private tmux server** `sync-probe-lag-<pid>` (`-f /dev/null`). In it three real views (`cpd tui view tree`, `plan`,
  `widgets`, window `v`, widths 34, 216 and 33 columns, height 50) and the real engine (`cpd watch`, window `e`), each
  under `timeout`. Never `/home/clayton/.cpd`, never the socket `cpd` (verified: the harness asserts both prefixes).
- The store is **seeded**, not a copy of the live one: Context `ctx`; Plan `pay` (deploy) with one open PR, its
  `pull_requests` row and Lead `lead1` in the chat slot; Plan `ghost` with 30 draft Jobs (for trigger Q).
  It is small (tens of rows): read cost on live-size data is `panel-sync-cost.md`'s (section 2 there), not measured here.
- **T0** = the `at` of the `txns` row of the commit (written by the writer inside the transaction, ms resolution), read
  back from the store. **T1** = the middle of the `tmux capture-pane -p` call (each pane polled in its own thread every
  20 ms) that first shows the expected text. The predicate is a regex per surface.
- **Resolution**: one observer sample is 1.8 ms median per capture (n about 17 000 per observer, p95 3.0 ms, max up to
  93 ms under load). The previous sample's start is exactly 20 ms before the hit's start (n=40, min = max = 20 ms), so T1
  is good to about +/- 12 ms. T0 is the stamp inside the transaction: the commit lands up to a few ms later; the CLI
  commands (triggers A, B, Q-down) take 23 to 32 ms in all (n=40).
- **Build**: `target/debug/cpd`, the `dev` profile (unoptimized), `0.13.45-dev`, built 17:16 from the `lag-a1`
  worktree. Its commits on top of `bf1214f7` change only the findings file (verified, `git log --stat`), so the code is
  the Lead Branch's. The live cockpit runs a release build: debug code is slower to read and draw. The latencies
  below are timer-dominated (hundreds of ms), so the effect is small (guess); the 2 to 4 ms read would be less in release.
- **Poll period and its cost** (from the straces `s_<view>.txt`, `fcntl` read-mark locks on the follower thread):
  tree and plan check the store every 252 and 257 ms on average (intervals alternate 200 and 300 ms: the 200 ms sleep
  is rounded up to the next 100 ms frame; n=3 950 and 3 834 intervals); widgets every 505 ms (n=1 992) on its **loop
  thread**. An idle check holds the read mark 0.17 to 0.18 ms (median; n=4 709 and 8 332; under strace, so an upper
  bound). The CPU of the views is `panel-sync-cost.md` section 1 (tree 0.37 %, plan 0.36 %, widgets 0.14 %).
- **Machine load and nice 19** (hs-d528): the probe's processes (views, engine, observers) all ran at nice 19; the
  live cockpit does not. `/proc/loadavg` (1 min) at the start and end of each run and the lowest and highest 1-min value
  read before a commit:

| Run | Local time | loadavg start to end | per-commit min to max |
|---|---|---|---|
| A-bug | 17:23 to 17:30 | 0.73 to 0.85 | 0.73 to 1.40 |
| A-main | 17:30 to 17:36 | 0.89 to 1.41 | 0.82 to 1.69 |
| B-bug | 17:36 to 17:54 | 1.30 to 10.90 | 0.80 to 24.66 |
| B-bug2 | 17:55 to 18:12 | 17.10 to 2.03 | 0.81 to 16.85 |
| B-main | 18:14 to 18:19 | 1.49 to 8.88 | 1.38 to 11.73 |
| C-main | 18:19 to 18:24 | 10.33 to 1.15 | 1.07 to 11.46 |
| Q-main | 18:24 to 18:29 | 1.05 to 0.67 | 0.52 to 1.19 |
| A-strace | 18:30 to 18:33 | 0.69 to 1.12 | 0.59 to 1.25 |
| C-strace | 18:33 to 18:37 | 1.03 to 1.26 | 0.71 to 1.47 |
| Q-outer | 18:39 to 18:44 | 0.48 to 1.19 | 0.55 to 1.19 |
| T | 18:44 to 18:47 | 1.00 to 2.72 (per commit) | 1.00 to 2.72 |
| fresh A | 18:49 to 18:55 | 1.19 to 1.34 | 0.92 to 2.68 |
| fresh B | 18:55 to 19:00 | 0.78 to 1.45 | 0.74 to 1.54 |

  Effect of nice 19: B-main and C-main ran at 1-min loads up to 11.7 on 16 cores; their capture cost rose (p95 9.0 ms
  against 3.0 ms) but the view hop did not: tree draw minus engine event is 337 and 339 ms median in B-main (n=20 each)
  at load up to 11.7 and 337 ms in the fresh B (n=40) at load 1.5. The engine's own timing was not delayed either (ticks
  5 000 to 5 018 ms in 12 runs, n=1 046 intervals). No run shows a wakeup waiting for a nice-0 process (a delay of more than a
  frame would show; none did); the 16 cores were not saturated (guess: nothing runnable waited).

Percentiles are linear interpolation over the n samples (`derive.py`). "seen/n" is the share of commits whose text
appeared before the 22 s deadline (6 s fixed window for widgets in A).

## 2. Results per trigger

Triggers (each alternates two commits, 20 each; gaps between commits uniform 1 to 4 s plus 0 to 5 s, random):

- **A** `cpd session state idle|working --session lead1` (a CLI write; Event `session-state`).
- **B** `cpd review open|close` (a Lead's review; the engine opens or closes an Inbox item at E8).
- **C** SQL `UPDATE pull_requests SET mergeable=..., merge_state=..., changed_at=now` (what `pr poll` writes: no
  Event, section 6); the engine closes (conflict) or opens (clean) an Inbox item (guess: the `pr-ready` kind; the
  harness tests only the tree's Inbox count).
- **Q** a Job `draft` to `queued` by SQL with its `job-added` Event (up), then `cpd job cancel` (down); surfaces: the
  tree's `ghost N` count, the tmux option `@cpd_workers` the engine sets (`" · 1 queued"`).

### 2.1 Raw data re-derived (previous Attempt, main runs)

| Trigger and commit | Surface | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|
| A idle (A-main) | plan | 20/20 | 220 | 337 | 478 | 489 |
| A idle | widgets | 0/20 | - | - | - | - |
| A working | tree | 20/20 | 235 | 359 | 479 | 511 |
| A working | plan | 20/20 | 209 | 348 | 445 | 472 |
| A working | widgets | not waited for (see below) | | | | |
| B open (B-main) | tree (Inbox 1) | 20/20 | 731 | 2 390 | 5 213 | 5 310 |
| B open | engine's `inbox-opened` Event (`at` minus T0) | 20/20 | 190 | 2 147 | 4 896 | 4 973 |
| B close | tree (Inbox 0) | 20/20 | 449 | 2 410 | 4 965 | 4 997 |
| B close | engine's `inbox-closed` Event | 20/20 | 197 | 2 062 | 4 630 | 4 753 |
| C conflict (C-main) | tree (Inbox row) | 19/19 | 438 | 2 715 | 4 933 | 5 009 |
| C conflict | plan | 20/20 | 227 | 350 | 447 | 455 |
| C conflict | widgets | 20/20 | 59 | 305 | 442 | 499 |
| C conflict | engine's Event | 19/19 | 23 | 2 360 | 4 505 | 4 674 |
| C clean | tree (Inbox row) | 20/20 | 918 | 3 522 | 4 716 | 4 936 |
| C clean | plan | 20/20 | 247 | 350 | 481 | 497 |
| C clean | widgets | 20/20 | 39 | 221 | 479 | 493 |
| C clean | engine's Event | 20/20 | 288 | 3 183 | 4 470 | 4 708 |
| Q up (Q-main) | tree (`ghost` count) | 20/20 | 225 | 340 | 455 | 455 |
| Q up | tmux option `@cpd_workers` | 20/20 | 136 | 2 621 | 4 578 | 4 710 |
| Q down | tmux option `@cpd_workers` | 20/20 | 82 | 2 533 | 4 658 | 4 669 |

The `n` of each cell is the `seen` count; "C conflict tree 19/19" is 19 commits with a tree predicate (the first has
none: no item to close yet). The A "working" widgets cell: the predicate was **already true before the commit in 20 of
20** (the pane showed a spinner from the seed's `working` Lead and never updated), so the harness did not wait for it;
see 2.4. The `Q-outer` run (same commits, Q) gives the same: tree 226 to 510 (median 354, n=20), option up 410 to
4 567 (median 2 738), option down 604 to 4 651 (median 2 409); with the outer tmux in section 5.

### 2.2 The other runs (previous Attempt)

| Run | What | Result (median, min to max, n) |
|---|---|---|
| A-bug | first A run, 17:23 | plan idle 304 (212 to 519, 20), tree 351 (220 to 534, 20), plan working 345 (213 to 489, 20), widgets 0/40 |
| A-strace | A with strace on the views | plan idle 322 (241 to 518, 10), plan working 311 (209 to 456, 10), widgets 0/10 |
| C-strace | C with strace | tree 3 537 (919 to 4 995, 14) and 2 306 (1 414 to 5 141, 15); plan 323 and 333 (215 to 512, 15 each); widgets 309 and 220 (32 to 513, 15 each) |
| B-bug, B-bug2 | B with a defect | see 2.4: tree 0/40 in both; engine Events 1/20 and 19/20 |
| T | tmux probe (`set -p @cpd_title`, outer view) | 26/26 seen, 2 to 21 (median 7), n=26; the run was killed at 26 of 40 commits, no summary line |

### 2.3 Fresh run (this Job), same harness, same binary

New private socket `sync-probe-lag2-<pid>`, new throwaway home, seeded as above (the old worktree's `target/debug/cpd`,
the same binary as the raw runs: this checks the harness and the machine, not the build).

| Trigger and commit | Surface | seen/n | min | median | p95 | max |
|---|---|---|---|---|---|---|
| A idle | plan | 20/20 | 214 | 309 | 479 | 488 |
| A idle | widgets | 0/20 | - | - | - | - |
| A working | tree | 20/20 | 219 | 339 | 491 | 522 |
| A working | plan | 20/20 | 209 | 327 | 442 | 449 |
| B open | tree | 20/20 | 304 | 2 018 | 4 373 | 4 780 |
| B open | engine Event | 20/20 | 101 | 1 675 | 3 970 | 4 546 |
| B close | tree | 20/20 | 375 | 2 476 | 5 171 | 5 214 |
| B close | engine Event | 20/20 | 26 | 2 128 | 4 847 | 4 966 |
| T (tmux title, outer view) | outer pane, top line | 40/40 | 1 | 11 | 20 | 22 |

Agreement: A plan and tree agree with the raw data within 28 ms of median and 40 ms of max (plan idle 309 against 337
and 304; tree 339 against 359 and 351; plan working max 449 against 472 and 489), and widgets are 0/20 again: **agreement**. B: the medians differ by up to
372 ms (tree open 2 018 against 2 390) and the engine Event by 472 ms. Cause: the wait for the next tick is uniform over
0 to 5 s (tick starts exactly every 5.000 s), so a median of 20 draws has a standard error of the median of about 0.3 to 0.4 s (SD 1.4 s of a uniform 0 to 5 s, n=20);
the 40-draw hop after the engine is the same: tree minus engine Event 337 ms median (n=40, min 132, max 449) in the fresh
run against 337 and 339 (n=20 each) in the raw data. The engine's Event lands 12 ms (11 to 16, n=40) after the tick
start in the fresh run, 12 ms (11 to 23, n=40) in the raw. **Agreement.**

### 2.4 What the `-bug`, `-strace` and `-main` files were about

The previous Attempt left no notes; what the files show:

- `A-bug` is trigger A, first run (17:23 to 17:30), before `A-main` (17:30 to 17:36). Plan and tree agree with `A-main`
  and the fresh run (medians 304 and 351 against 337 and 359). The only difference is the widgets surface (0/40 in
  `A-bug`, "already true" in 20/20 of `A-main`); the harness's widgets predicate was changed in between (guess: the
  file does not say which). The widgets result of section 2.1 does not depend on it: the fresh run and a hand check
  (section 2.5) agree.
- `B-bug`: the tree never showed the Inbox count (0/20 per direction) and the engine wrote an `inbox-*` Event in 1 of
  20 commits: after the 4th commit no engine transaction appears between the probe's commits (the `txn` ids run
  87, 89, 91, 92, 93, ..., verified in `B-bug.jsonl`), while the ticks continued. Load went to 24.7 (a build of another
  Worker). Cause not recorded; guess: the review's `--pn` key was reused, so there was no new item to open.
  `B-bug2` (17:55 to 18:12): the engine acted in 19 and 18 of 20 commits, but the tree still showed 0/20; the throwaway
  home and the views were rebuilt at 18:13 (`home.txt`, 18:13) and `B-main` then gave 20/20. Cause not recorded (guess:
  the views of the old home had been running 50 min and reached their `timeout 3000`, or the Inbox-count observer was
  fixed in between; unverified). None of the three is a defect of cpd that I could find; `B-main` and the fresh B are the data.
- `A-strace` and `C-strace`: triggers A and C again with `strace` on the three view processes (the `s_<view>.txt`
  files: `pread64`, `fcntl` and `write` lines with epoch timestamps to the microsecond), to split the latency into
  hops (section 4). The totals agree with the runs without strace (plan A idle 322 against 337, plan C 323 and 333
  against 350 and 350; n=10 to 15), so strace did not visibly slow the loops; only the read hop is inflated. In
  `A-strace` the tree and widgets predicates were already true before every working commit (10/10), so only plan is
  reported there (cause unestablished).

### 2.5 The widgets view and a Lead's state (hand check, 1 run)

In the fresh store: `session state working` then 8 s of captures every 1 s: the widgets row stayed `open 󰮔 ...` (no
spinner); then `UPDATE pull_requests SET changed_at=now` (no Event) and the spinner appeared 0.6 s later (frames 󰽨, 󰽣,
󰽧 in 0.3 s steps). Source: `tui/src/widgets/mod.rs` `KINDS` holds `pr-opened`, `plan-created`, `session-registered`,
`session-moved`, `job-added`, `attempt-started`, `job-retried`, `job-merged`, `job-failed`, `job-cancelled` and no
`session-state`; `open_prs` (which holds `lead_working`) is read only when `poll_changes` says something moved. The
tree and plan views do list `session-state`.

## 3. Spread between views

For the same commit, the largest minus the smallest T1 over the surfaces that drew it (n commits, ms):

| Commit | Surfaces | n | min | median | p95 | max |
|---|---|---|---|---|---|---|
| A working (A-main) | tree, plan | 20 | 1 | 103 | 211 | 212 |
| A working (fresh) | tree, plan | 20 | 4 | 88 | 211 | 213 |
| C, both directions (C-main) | plan, widgets | 40 | 1 | 135 | 273 | 412 |
| C conflict (C-main) | tree Inbox row, plan, widgets | 20 | 22 | 2 443 | 4 750 | 4 859 |
| C clean (C-main) | tree Inbox row, plan, widgets | 20 | 724 | 3 360 | 4 526 | 4 816 |
| Q up (Q-outer) | tree, `@cpd_workers`, outer tmux | 20 | 18 | 2 345 | 4 177 | 4 190 |
| Q down (Q-outer) | option, outer tmux (2 surfaces) | 20 | 2 | 7 | 18 | 18 |

Two views fed by the store directly (tree and plan, plan and widgets) differ by up to 0.2 s and 0.4 s: their polls
and frames are not phase-locked. A view fed by the engine (Inbox row, `@cpd_workers`) differs by the engine's wait:
the median is the median tick wait.

## 4. Hops

Source: `A-strace`, `C-strace` (straces of the views in the private server; `hops.py` in the Appendix pairs the
probe's T0 and T1 with the view's own syscalls: the first database read burst after the change, the first burst with
at least 15 `pread64` (the snapshot), the next `write(1, ...)` (the draw)). ms; n is the commits.

Plan view, trigger A and C (median, min to max; n = 10 to 15 per cell, `A idle`, `A working`, `C conflict`, `C clean`):

| Hop | A idle | A working | C conflict | C clean |
|---|---|---|---|---|
| tick phase (the engine) | none: a CLI/SQL write | none | none | none |
| write to the poll that sees it | 109 (33 to 301) | 92 (2 to 240) | 115 (9 to 294) | 125 (14 to 272) |
| poll to the snapshot's first read (coalescing) | 100 (100 to 101) | 100 (99 to 101) | 100 (99 to 101) | 100 (99 to 101) |
| read (strace-inflated) | 3 (2 to 4) | 2 (2 to 4) | 2 (2 to 4) | 3 (2 to 3) |
| read end to the draw | 97 (95 to 99) | 97 (96 to 99) | 98 (96 to 99) | 98 (96 to 99) |
| draw to the capture that shows it | 9 (1 to 18) | 11 (2 to 19) | 9 (1 to 18) | 11 (1 to 18) |
| total from T0 | 322 (241 to 518) | 311 (209 to 456) | 323 (215 to 512) | 333 (216 to 488) |

The tree row of C (`C conflict`, `C clean`, n=14 and 15) is the same: 116 and 115 to the poll, 99 and 100 to the snapshot, 2
read, 99 and 99 to the draw, 6 and 9 to the capture; total 320 and 318 from the engine's Event (its Inbox row waits
for the engine's write).

Widgets view (C, n=15 per cell): write to the snapshot's first read 289 (27 to 496) and 204 (32 to 440) (its poll is on
the loop thread, every 505 ms, so uniform 0 to 505); read 2 (1 to 3); read to draw 0 (0 to 1) (the same thread
draws after the read); draw to capture 14 and 11; total 309 and 220 from T0.

Reading the table: the poll wait is 0 to 300 ms (the poll period) and then **two frame boundaries of about 100 ms**:
the loop takes the poll's answer at the next frame boundary and starts the snapshot, and takes the snapshot's result
at the next frame boundary and draws it (99 to 101 ms and 95 to 99 ms, n=79 each: A idle 20 plan, C 30 plan and 29 tree). The read itself is 2 to
4 ms under strace in a debug build. "Coalescing" in the sense of the 500 ms `COALESCE` / `SNAPSHOT_EVERY` was **not
exercised**: every commit came at least 1 s after the previous one (gap 1 to 9 s), so the 500 ms rule never held a
snapshot back; a burst test was not run (not measured).

Engine hop (tick phase), B, C and Q (the engine's step and the tick it belongs to, from `watch.lock`'s mtime, which the
engine touches at each tick's start (`engine.rs`: `lock.touch()` before `tick()`)):

| Run | Surface | Event or option minus the next tick's start (ms): n, min, median, p95, max |
|---|---|---|
| B-main | `inbox-*` Event | 40, 11, 12, 19, 23 |
| C-main | `inbox-*` Event | 39, 11, 12, 14, 14 |
| C-strace | `inbox-*` Event | 29, 11, 12, 13, 14 |
| fresh B | `inbox-*` Event | 40, 11, 12, 14, 16 |
| Q-main | `@cpd_workers` option seen | 40, 11, 20, 30, 31 |
| Q-outer | `@cpd_workers` option seen | 40, 13, 24, 31, 31 |

Ticks: 5 000 to 5 018 ms apart in the 12 runs with a summary (n=1 046 intervals; E5 off). The wait from a cause to
the next tick start is uniform: 11 to 4 961 ms (median 2 132 to 2 983 ms, n=29 to 40 per run). So in the probe:
cause to Inbox row = wait to the next tick (0 to 5 s) + 12 ms (engine) + 337 ms (view: median, n=80; 217 to 624 ms).

## 5. tmux

How tmux shows an option the engine sets: **measured** in the probe, with a second private tmux server `outer`
(`sync-probe-lag-<pid>-outer`, `tmux -L <outer> new-session -d -s o "tmux -L <inner> attach -t x"`) showing the inner
server's screen; the inner's pane border format reads `T=#{@cpd_title}` (`T`) and the inner's status line read
`@cpd_workers` (`Q-outer`). The set command is `tmux -L <inner> set -p -t <pane> @cpd_title tN`.

| Probe | What | n | min | median | p95 | max |
|---|---|---|---|---|---|---|
| T (raw) | border text after `set -p` returns, outer capture, minus T0 | 26 | 2 | 7 | 20 | 21 |
| Q-outer | absolute difference of two observers' T1, outer top line against `show -gv @cpd_workers`, engine-set (20 up + 20 down) | 40 | 1 | 5 | 15 | 18 |
| fresh T | as T, load 0.43 to 1.02 | 40 | 1 | 11 | 20 | 22 |

The option is drawn on the next redraw, within one observer period (20 ms) of being set: **not a source of seconds**
(a median of 7 and 11 ms in two runs is the observer's own 0 to 20 ms sampling wait plus the capture). In Q-outer the
border text and the status text of the outer server both moved within 18 ms of the option read (n=40).
The engine sets `@cpd_workers` at the start of each tick (first step after the restart drain): measured as 20 ms median
after the tick start (11 to 31 ms, n=80), so its lag is the tick wait. Titles (`@cpd_title` of a Lead's pane) are
written by `titles::run`, which spawns a process per session and only when a turn is older than 10 minutes (read
in `titles.rs`, `due()`); their write cost and path were not measured. The real cockpit's tmux config (status off,
`pane-border-status top`, the format in `core/src/host/tmux.rs`) is not the probe's inner config (unverified that it
redraws as fast as the probe's `T=#{@cpd_title}` border).

## 6. The GitHub path (E5)

- **By reading** (`engine.rs`, `pr.rs`): the engine runs 14 steps per tick; E5 is the 7th, `pr::run`, E8 (`inbox::run`)
  the 9th, so E8 comes after E5 in the same tick. E5 runs `cpd pr poll` as a child and waits for it (deadline 120 s),
  only when `pr_poll_seconds` (5) have passed since the previous poll ended; the next tick starts at the earlier of
  tick start + 5 s and E5's due time, so a tick that ran a 6 s poll is followed at once by the next tick. `pr poll`
  writes `pull_requests` with no Event (contract 7.6); the views notice it by the `pull_requests` mark
  (`count(*)`, `max(changed_at)`) in the same poll as the Events. A poll that changes nothing writes nothing, and
  E5 logs (`run/watch.log`) only a poll that changed, fed back or merged something.
- **Cadence on the live engine, measured, read-only**: `/proc` scanned every 100 ms for 2 windows of 150 s
  (18:50 and 18:54 EDT, live engine pid 2438527, children whose argv is `pr poll`) and `locks/watch.lock`'s mtime
  stat every 50 ms (window 2). Poll children: 26 spans (13 + 13), duration 4.8 to 7.8 s (median 6.7 and 6.8 s; the
  100 ms scan makes it a lower bound +0.1 s), start to start 9.9 to 12.8 s (median 11.6 and 11.7 s; n=12 + 12). Ticks
  in window 2: 27 starts, 26 intervals 4.98 to 7.90 s (median 5.01 s, mean 5.75 s). So the cycle is: a tick starts,
  E5 blocks it for about 6.5 s, the next tick starts at once (E5 not due), the third starts 5 s after the poll ended and
  polls again. Nothing in `/proc` says what `gh` did inside a poll (the 5 to 8 s is the child's wall time, unverified
  split between `gh` calls and the store). The load (1 min) was 1.2 to 1.4; a poll's wall time may depend on GitHub and on the
  machine (not varied).
- **E8's times, derived (unverified, E8's own moment was not observed)**: if E8 runs right after E5 returns, E8
  runs at about poll end (poll start + 6.5 s) in the poll's tick and 10 ms later in the next tick; the next one is
  11.5 s later. From the 27 tick starts and 13 poll spans: 26 gaps between E8 runs, 13 below 0.2 s and 13 from 9.9 to
  12.8 s, mean residual wait of a cause E[g^2]/(2 E[g]) = 5.70 s, longest gap 12.83 s. This fits
  `panel-sync-cost.md` section 4 (median 6.1 s, p95 12.6 s, n=229 for `dialog` Inbox items), which is a different
  measurement of the same thing from history.
- **In the probe E5 was off** (`pr_poll_seconds = 4294967295`, `gh` stubbed), so the GitHub path is measured from the
  `pull_requests` commit onward only: the PR change reached plan and widgets in 215 to 513 ms and the Inbox row after the
  next tick (C). The age of the GitHub data itself (up to one poll cycle of about 11.5 s plus GitHub's own latency) is
  not measured.

## 7. Verdict on the handoff's inferred claims (section 3 of `tui-architecture-handoff.md`)

| Claim | Verdict | Evidence |
|---|---|---|
| A view sees a change within about 0.7 s | **Confirmed** for tree and plan: max 534 ms (n=40 per view per run, 3 runs) | 2.1 to 2.3 |
| ... for the widgets view | **Confirmed** for `pull_requests` changes (32 to 513 ms, n=70); **corrected** for a Lead's state: never drawn (0/70 in 6 s) until another Event or PR change | 2.1, 2.5 |
| The tree and plan view check every 200 ms, snapshot at most every 500 ms | **Corrected in detail**: the check is every 200 or 300 ms (mean 252 ms, n=3 950), widgets 505 ms; the snapshot starts 100 ms after the poll, no 500 ms hold in 1 to 9 s gaps (the burst case is not measured) | 1, 4 |
| The engine's 5 s tick includes Inbox items | **Confirmed**: item opened or closed 12 ms (11 to 23, n=148) after a tick's start; tree shows it median 337 ms later | 4 |
| ... includes Job transitions | **Unverified**: the harness's Job write (Q) was a store write; no engine-derived Job transition (E3, E4) was triggered | - |
| ... includes engine-set tmux options | **Confirmed** for `@cpd_workers`: 20 ms median after the tick start (n=80); tmux draws it within 20 ms | 4, 5 |
| The Inbox is up to about 5 s later | **Confirmed** as a bound in the probe (cause to Inbox row max 5.3 s, n=148, E5 off); **corrected** on the live cockpit: E5 blocks the tick for about 6.5 s, so the gap between E8 runs is up to 12.8 s and the lag median about 6 s (cost.md: 6.1 s, p95 12.6 s) | 4, 6 |
| GitHub data arrives at the PR poll every 5 s | **Corrected**: 5 s after the previous poll *ends*, and the poll takes 4.8 to 7.8 s, so polls start 9.9 to 12.8 s apart (n=24) | 6 |
| ... slower after transient errors | **Not measured** | - |
| 0.1 to 0.4 % CPU per view; under 10 messages/s | Not part of this Job; `panel-sync-cost.md` section 1: tree 0.37 % median (worst 0.90), plan 0.36 %, widgets 0.14 % | cost.md |

## 8. Where the seconds come from

Ranked by measured share of the commit-to-draw time of the surface that waits longest:

1. **The engine's tick wait and E5** for anything the engine derives (Inbox rows, `@cpd_workers`): in the probe the
   median cause-to-row time is 2.0 to 3.5 s, of which the tick wait is 2.1 to 3.0 s (about 85 to 90 %) and the view hop
   0.34 s (10 to 15 %). On the live cockpit E5 adds the 6.5 s blocking poll to every other tick: mean wait 5.7 s,
   up to 12.8 s (section 6, derived).
2. **The widgets view never redraws a Lead's state change** (0/70 in 6 s): unbounded until the next listed Event or PR change.
3. **Frame waits of the loops**: 2 x 100 ms in the tree and plan views (about 60 % of their median 310 to 330 ms:
   100 ms poll answer to snapshot, 97 ms read to draw), the poll wait 0 to 300 ms (median about 110 ms, 34 %), the read 2 to
   4 ms (1 %) and the draw to a visible capture below 20 ms.
4. **Phase between views**: up to 0.2 s (tree against plan) and 0.4 s (plan against widgets) from independent polls.
5. tmux: below 20 ms (section 5).

## 9. Not measured, unverified

- Read cost and draw on live-size data (cost.md section 2 has the reader CPU per commit); this store is a seed of tens of rows.
- Coalescing under bursts (the 500 ms `COALESCE` / `SNAPSHOT_EVERY`): every commit in the harness was at least 1 s apart.
- The E8 moment itself on the live engine (derived from tick starts and the poll's span, unverified); E8 behind `pr poll`
  in a probe engine (E5 was off); what a poll's 5 to 8 s is made of.
- Release build: all probe runs are a debug build (the live cockpit's views are release).
- Job transitions, Inbox kinds other than the ones B and C trigger, titles (`@cpd_title` by `titles::run`) and their cost,
  the real cockpit's tmux config, the pi chat pane.
- The `B-bug`, `B-bug2`, `A-bug` causes (section 2.4), and why the tree and widgets predicates were already true in `A-strace`.
- Only one afternoon, one machine; loads of 0.5 to 25 (per-run table in 1); nothing was run at nice 0.
- The previous Attempt's runs are labelled `previous Attempt` and were re-derived from its files, not re-run, except
  A and B and T (fresh).

## Appendix: repeat it

APPENDIX_PLACEHOLDER
