# Anim-perf: after the loader fix (Job stutter-remeasure, attempt 1)

## Summary

Method as in [anim-perf-plan.md](anim-perf-plan.md) (plan, release) and [anim-perf-tree.md](anim-perf-tree.md) (tree, debug). The causes are the plan file's H1, H3, H4, H2 (the Job names them C1, C2, C3, reads).

- **C1, the Lead dot freezes the plan view: fixed.** A1 (Lead working, no running Job): 1 draw in 35 s before, **382 draws**, gap p50 **100 ms** (p95 104, max 118) after.
- **C2, re-layout of 46 PR boxes at every frame: fixed.** `layout_us` above 0 on 100 % of draws before (p50 13.3-18.4 ms, p95 32-58 ms); after, **1 line of 381-382** in A1, A2 and B (the first draw, 18-30 ms). The data did not change in the runs, so 1 snapshot: unverified, the trace has no snapshot field. CPU 11.7-12.3 % to **2.1-2.3 %** (A2, B).
- **C3, fixed 100 ms sleep skips frames: fixed.** Skipped frames 56 of 379 (A2) and 83 of 380 (B) before, **0 of 380 / 381** after; the tree 25 of 366 (A) and 42 of 370 (B), **0 of 381** after. Gap p50 115-121 ms (plan), 105.8-106.8 ms (tree) before, **100 ms** after.
- **Reads on the draw thread (H2): fixed in effect.** `read_us` p50 0.30-0.37 ms (plan), 0.8-0.9 ms (tree) before; **0.001-0.002 ms** after (reads run on `pace::Reads`; the trace field now only times the hand-over). Plan gaps above 150 ms: 3 (A2) and 34 (B) before, **0** after. The store was never the cause (before) and a write-locked store (B) still changes nothing: max gap 105 ms (plan), 105 ms (tree).
- **Not fixed / still open:** tree CPU with a spinner is 3.5 % (A) and 4.9 % (B) in a debug build, above the 3 % line (before 4.4-4.5 %). See Left.

## Before and after

Gaps in ms over all draws but the first; "before" copied from the two files. Skipped = frames skipped / frame changes. Share = gaps above 150 ms / above 500 ms. layout, read in ms, non-zero values (read after: µs-scale).

Plan view, release build, Plan `quality`:

| Scenario | draws | gap p50 / p95 / max | > 150 / > 500 ms | skipped | CPU % | layout p50 / p95 | read p50 / p95 |
|---|---|---|---|---|---|---|---|
| A1 before | 1 | 715 (the first) | - / frozen | - | 0.20 | 43.7 (1 value) | 99.7 (1 value) |
| A1 after | 382 | 100 / 104 / 118 | 0 / 0 | 0 of 381 | 2.17 | 17.8 (1 value, first draw) | 0.001 / 0.003 |
| A2 before | 323 | 115 / 135 / 209 | 0.9 % / 0 | 56 of 379 | 11.7 | 13.3 / 32.1 | 0.30 / 0.40 |
| A2 after | 381 | 100 / 104 / 128 | 0 / 0 | 0 of 380 | 2.26 | 30.0 (1 value, first draw) | 0.001 / 0.003 |
| B before | 297 | 121 / 165 / 212 | 11.4 % / 0 | 83 of 380 | 12.3 | 18.4 / 58.1 | 0.37 / 0.51 |
| B after | 382 | 100 / 103 / 105 | 0 / 0 | 0 of 381 | 2.14 | 28.9 (1 value, first draw) | 0.001 / 0.003 |
| D before | 1 | 663 (the first) | - / - | - | 0.20 | 28.8 (1 value) | 51.2 (1 value) |
| D after | 2 | 661 (the second, at start) | - / - | - | 0.13 | 20.5 (1 value) | 0.008 (1 value) |

Tree view, debug build, Plan `quality`'s Lead working:

| Scenario | draws | gap p50 / p95 / max | > 150 / > 500 ms | skipped | CPU % | layout p50 / p95 | read p50 / p95 |
|---|---|---|---|---|---|---|---|
| A before | 367 | 105.8 / 115.4 / 124.1 | 0 / 0 | 25 of 366 | 4.45 | none (0 lines) | 0.8 / 1.3 |
| A after | 382 | 100 / 101 / 103 | 0 / 0 | 0 of 381 | 3.54 | none (0 lines) | 0.002 / 0.005 |
| B before | 371 | 106.8 / 132.6 / 175.4 | 1.6 % / 0 | 42 of 370 | 4.4 | none | 0.9 / 1.3 |
| B after | 382 | 100 / 101 / 105 | 0 / 0 | 0 of 381 | 4.86 | none | 0.001 / 0.004 |
| D before | 38 | 1010.7 / 1063.4 / 1068.6 | 100 % / 100 % | - | 1.0 | none | 4.9 / 30.4 |
| D after | 39 | 1000 / 1001 / 1001 | 100 % / 100 % | - | 0.86 | none | 0.024 / 0.032 |

Draw time (ms, p50 / p95 / max), after: plan A1 0.53 / 4.1 / 10.7, A2 0.43 / 4.0 / 20.2, B 0.47 / 1.3 / 7.0; tree A 3.0 / 4.1 / 16.9, B 4.7 / 5.9 / 13.3, D 4.7 / 5.4 / 5.7. Before: plan 0.42-0.54 / 0.68-4.35 / 4.7-16.3; tree 4.7-5.1 / 12-26 / 21-74.

## Method

Only the differences. Same steps (copy of `~/.cpd/cpd.db` by python `backup`, all triggers dropped, a `session-moved` Event `{"place":"chat"}` for the Lead of `quality`, Lead `working`, one Job `running` in A2 and B, private `tmux -L anim-after`, 200x50, `CPD_TUI_TRACE=1`, 35 s windows, D 30 s plan / 35 s tree, B = `BEGIN IMMEDIATE` 5 x 3 s from t = 2 s).

- Worktree at the merged loader fix (`afe809b7`); release (plan) and debug (tree) built with `scripts/cargo.sh`, the profile each before file used.
- Every scenario ran on a fresh file copy of one prepared base (the Job and Session state set per scenario with `UPDATE`s), one run each, no repeat: single samples (the before files too).
- Tree scenarios use the Lead `working` alone (no running Job), like the tree's before runs; D has all Sessions `stopped`.
- Load average (1 min at the end of each run): plan 9.3 / 12.0 / 12.7 / 12.6 (A1, A2, B, D), tree 11.7 / 8.1 / 5.8; 15 min: 15-19 and 12-15. Before: plan 24-38, tree 7.5-17.9. Nice 19 in both before and after (the Worker session's), the real views run at 0. The machine was lighter in the plan runs now than before, so part of the plan gain is load: the 0 skipped frames, the p50 100 ms and 1 layout line are not.
- CPU from `/proc/<pid>/stat` (100 ticks/s) over the window after a 3 s wait. A "skipped" count is `(b - a) mod 8 - 1` over frame changes, as before.
- The first measured gap in A1, B (7, 21 ms) is the second draw at start, not a stutter; no `strace` (ptrace denied, as before).

## Left

- **Tree CPU with a spinner: 3.54 % (A), 4.86 % (B)**, debug build, nice 19, load 6-12: above 3 %. `draw_us` p50 3.0 / 4.7 ms is the whole cost; the release figure is unverified (not run). The plan view is at 2.1-2.3 % in release.
- **Tree draw tail:** `draw_us` max 16.9 ms (A), 13.3 ms (B); no gap above 105 ms.
- **Plan first draw:** layout 18-30 ms once, and D's second draw 661 ms after start (no spinner; no freeze while a spinner is on screen). Time to first paint was not changed by this fix.
- **Tree D:** 1 draw/s by design, 38-39 draws in 35 s, gap 1000 ms (above 500 ms with no spinner).
- Nothing else above 150 ms in any scenario with a spinner (max 128 ms plan A2, 105 ms tree B).
- Unverified: release tree, a store that blocks readers, the real nice-0 views at real load, repeat runs for variance, the snapshot count (the trace has none).
