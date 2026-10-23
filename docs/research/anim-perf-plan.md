# Plan view: stutter cause and hot paths (Job stutter-measure-plan, attempt 1)

## Summary

- **Freeze (H1, the User's "stops at one frame"):** a Plan whose Lead works with no running Job draws **1 time in 35 s** (first draw only; 1 distinct frame, gap 715 ms, then nothing). The PR title dots (`Pull::lead_working`) are on screen, frozen. `Centre::spins()` counts only running Jobs.
- **Slow/uneven (H4):** with a running Job the view draws at p50 115-121 ms, not 100 ms, and **skips 15-22 % of the frames** (56 of 379 / 83 of 380 frame advances skipped). Cause: the loop sleeps a fixed `poll(100 ms)` after each draw, so the period is 100 ms + work, against a wall-clock frame.
- **Work per frame (H3):** every frame re-lays out all 46 PR boxes: `layout_us` p50 13-18 ms, p95 32-58 ms (release), 100 % of draws, and it is what carries every gap above 150 ms (34 of 34 in B, 3 of 3 in A2). CPU 11-12 % of a core for one moving icon.
- **NOT causes:** a busy store (B: 5 x 3 s write locks, reads max 40 ms, same as A2: WAL readers are not blocked), draw time (p50 0.4-0.5 ms), reads (p50 0.3-0.4 ms, 0 frozen gaps), idle CPU (0.2 %). Nice value of the real view is 0 (my runs: 19, inherited, see H5).
- Nothing exceeded 500 ms while a spinner was redrawn (max gap 213 ms), so "stops at one frame" in the plan view is H1 alone.
- Build: **release** for all tables (debug measured too: A2 p50 140 ms, 30 % > 150 ms, layout p50 38 ms).

## Method

1. `scripts/cargo.sh build --release -p cpd`; run `target/release/cpd` by path. `COPY=$(mktemp -d)`, `TMUX_TMPDIR=$(mktemp -d)`, `unset TMUX`; `trap` kills the private server `tmux -L anim-plan`.
2. Copy of `~/.cpd/cpd.db` by python `sqlite3.Connection.backup` plus `config.toml`, `run/`, `locks/`. In the COPY only: drop all triggers (history triggers need the engine's `cpd_txn()`), insert one `session-moved` Event `{"place":"chat"}` for the Lead of the largest Plan so `chat_slot` shows it.
3. Plan: `quality` (213 Jobs: 147 merged, 66 cancelled; 50 PRs, 46 with a pull request, 4 cancelled). Lead (`plans.lead_session`) state set per scenario: **A1** `working`, no running Job (the PR title dots are the only spinner); **A2** `working` + the newest merged Job set `running` (and `merged_sha` NULL); **B** = A2 with the store busy; **D** Lead `stopped`, no running Job (no spinner). Each scenario resets the others first.
4. `tmux -L anim-plan -f /dev/null new-session -d -x 200 -y 50 -e CPD_HOME=$COPY -e CPD_TUI_TRACE=1 'cpd tui view plan'`; wait 3 s; read `/proc/<pid>/stat` utime+stime over the window (35 s A, 30 s D); `ps -o ni`; `kill-server`; trace lines from `$COPY/run/tui/plan.log`. Percentiles: nearest rank over all draws except the first; read/layout/draw over non-zero values.
5. B: python `BEGIN IMMEDIATE`; sleep 3; `COMMIT`, 5 times with 2 s between (all `held 3.0`), started 2 s into the window.
6. Skipped frames: from `frame=` of consecutive lines, `(b - a) mod 8 - 1` summed. Reads/s: `/proc/<pid>/io` syscr/syscw over 10 s.
7. Machine: 7 logins, load average 24-38 over the runs (other Jobs build in parallel), the shell and so the views at nice 19. `strace -p` is refused (ptrace not permitted): not run.

## Results

Release build, view of `quality`, gaps in ms (all draws but the first). "frozen" = gap > 500 ms while a spinner is on screen.

| Scenario | draws | gap min / p50 / p95 / max | > 150 ms | > 500 ms | CPU % | ni |
|---|---|---|---|---|---|---|
| A1 Lead works, no running Job | 1 in 35 s | 715 (the first) | - | frozen after draw 1 | 0.20 | 19 |
| A2 Lead works + running Job | 323 | 109 / 115 / 135 / 209 | 0.9 % (3) | 0 | 11.7 | 19 |
| B A2 + store write-locked 5 x 3 s | 297 | 110 / 121 / 165 / 212 | 11.4 % (34) | 0 | 12.3 | 19 |
| D no spinner | 1 in 30 s | 663 (the first) | - | - | 0.20 | 19 |

| Scenario | read_us non-zero: n, p50 / p95 / max ms | layout_us: n, p50 / p95 / max ms | draw_us: n, p50 / p95 / max ms |
|---|---|---|---|
| A1 | 1: 99.7 | 1: 43.7 | 1: 0.35 |
| A2 | 161: 0.30 / 0.40 / 39.8 | 323: 13.3 / 32.1 / 104.6 | 323: 0.42 / 0.68 / 4.72 |
| B | 148: 0.37 / 0.51 / 40.0 | 297: 18.4 / 58.1 / 91.2 | 297: 0.54 / 4.35 / 16.3 |
| D | 1: 51.2 | 1: 28.8 | 1: 0.44 |

Frames (A2 / B): 8 distinct frames; advance by 1: 267 / 214, by 2 (one skipped): 56 / 83, never more. The first draw reads and lays out the whole Plan: 100 ms read, 44 ms layout.

An earlier A2 on the same loaded machine, release, with the Job state wrongly left from A2 (a contaminated run, discarded) gave p50 132 / p95 234 / max 500 ms: the numbers move with machine load (layout p95 from 32 to 123 ms); read the tables above as typical, not worst.

Debug build (A1 1 draw / A2 257 draws, p50 140 / p95 194 / max 363 ms, 30 % > 150 ms; layout p50 38 ms, p95 87, max 261; CPU 24 %): the same shape, 2-3x slower.

## Cause

- **H1 confirmed.** A1: 1 draw in 35 s, 0.20 % CPU, 1 distinct frame, with the Lead dots on screen (screen capture shows the moon glyph after `merged`). With a running Job (A2): 323 draws, 8 frames. `Centre::spins()` must also count `pull.lead_working` (and any other spinner).
- **H2 refuted for a busy store, partly for the read cost.** B (5 x 3 s write lock): read max 40.0 ms vs 39.8 ms in A2, no gap above 212 ms, 0 above 500 ms. A WAL reader is not blocked by a writer; the view never waits for `store_busy_ms` (5000). Unverified: a store that blocks readers (checkpoint at a full stop, a non-WAL file, a lock held by an `EXCLUSIVE` open). What is true: reads run on the draw thread; p50 0.3-0.4 ms (the 200 ms Event poll, 140-160 per 38 s), the snapshot 40 ms in A2/B and 100 ms at start, each one delaying the next draw by its length.
- **H3 confirmed.** `boxes::layout` runs at every frame for all 46 PRs (13 ms p50, 32-58 ms p95, up to 105 ms); it is the largest field of all 37 gaps above 150 ms (A2 3, B 34) and its mean in them is 62-80 ms. Mean layout of the fast draws: 15-21 ms. `draw_us` is 0.4-0.5 ms: ratatui's diff is cheap, the cost is the layout.
- **H4 confirmed.** Minimum gap 109-113 ms, p50 115-121 ms: the 100 ms `poll` starts after the draw. 15 % (A2) and 22 % (B) of the frames advance by two: the spinner shows 6-8 distinct steps per second at uneven intervals. With no spinner the loop still wakes every 100 ms (`poll.min(FRAME)`): 0.20 % CPU, fine; `poll` (200 ms) rules only the Event reads.
- **H5 refuted for the real view, applies to my runs.** The real cockpit's plan and tree views run at nice 0 (`ps`: pids 4171796, 4171800); mine ran at 19 under load 24-38 (inherited from the Worker session), so my numbers are pessimistic. The real machine has the load (other Jobs), which the User sees.
- New: **the first draw costs 0.66-0.73 s** (read 100 ms + layout 44 ms + terminal setup): the view starts blank.

## Fix proposal

1. **Redraw rule:** `spins()` = any running Job **or** any PR with `pull.lead_working`, and for the Pulse/Manager view the same for any spinner it draws (none today, hs-d327). One function that names every spinner the drawing code emits, with a test per spinner (a frozen spinner is the failure that was invisible).
2. **Do not re-lay out per frame:** lay out when the snapshot or the width changes; at a frame change only substitute the icons (the layout is independent of the frame: draw the spinner glyph at draw time from `palette::frame`), or cache `Boxes` per frame. Removes 13-58 ms and ~11 % CPU per frame and the 150 ms gaps.
3. **Schedule by the clock:** sleep until the next 100 ms boundary of `palette::frame` (`poll(100 - now_ms % 100)`) instead of a fixed 100 ms, so a draw's work does not push the next draw past a frame; target: 0 skipped frames when the draw takes less than 100 ms.
4. **Reads off the draw thread:** run `snapshot`, `poll_changes` and the pull read in a worker (the view already has `Background`, tui-W7), send the new snapshot by channel; the draw thread only draws. Not needed for the measured stalls (40-100 ms) but needed for a store that blocks (unverified, H2).
5. Measure again with the same method after each change; add `strace` where permitted.

## Audit candidates

| Path | Measured | Verdict |
|---|---|---|
| Re-layout of 46 PR boxes at every spinner frame | 13 ms p50, 32-58 ms p95, 100 % of draws, 11-12 % CPU | slow |
| Fixed `poll(100 ms)` after the draw | skips 15-22 % of the frames | slow |
| Lead dot not in `spins()` | 1 draw in 35 s | slow (broken) |
| First draw (snapshot read 100 ms, layout 44 ms) | 0.66-0.73 s to first paint | slow (once) |
| Snapshot re-read (`plan_boxes`, 46 PRs) | 40 ms (A2/B), 100 ms at start; when a Plan Event arrives | slow-ish: one frame lost per Event |
| Event poll (`poll_changes`) | p50 0.3-0.4 ms, every 200 ms | fine |
| Terminal draw (ratatui diff) | p50 0.4-0.5 ms, p95 0.7-4.4 ms, max 98 ms (loaded run) | fine |
| Idle CPU, no spinner (D) | 0.20 % | fine |
| Writes while idle (D) | 0 write syscalls in 10 s; A2 31/s (the frame writes) | fine |
| Reads per second | unverified: syscr unchanged over 10 s (SQLite reads by mmap); the 200 ms Event poll appears as 5 non-zero `read_us` per s while drawing | unverified |
| Busy store (3 s write lock) | no effect on reads (max 40 ms) | fine |
| Scroll-offsets save (`offsets.save`) | not separately measured | unverified |
