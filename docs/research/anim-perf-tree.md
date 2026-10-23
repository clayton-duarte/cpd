# Anim-perf: Context tree view (`cpd tui view tree`)

## Summary

- **The tree view did not stutter in any scenario I could build**: with a spinner on screen, `gap_us` p50 105.8 ms, p95 115.4 ms, max 124.1 ms, 0 gaps over 150 ms (A, idle); p95 132.6 ms, max 175.4 ms, 1.6 % over 150 ms, 0 over 500 ms (B, store write-locked 5 x 3 s).
- **No freeze (gap > 500 ms) while a spinner is on screen** in A or B. The loop is the tree's `event::poll(TICK = 100 ms)` + draw whenever `palette::frame` changed; it drew on every ~106 ms.
- **H2 (reads block the draw) is not reproduced for the tree**: reads are 0.8 ms p50, 1.3 ms p95, 12.4 ms max (A); 11.1 ms max with a writer holding `BEGIN IMMEDIATE` for 3 s (B). SQLite WAL readers do not wait on a writer.
- **The only slow tail is `draw_us`**: p95 12.0 ms (A), 26.2 ms (B), max 74.1 ms; it is the one field that carries the tail gaps (top gaps 150-175 ms have draw 24-39 ms).
- **Not a cause on the tree**: H1 and H3 (plan view only), reads (1 ms), layout (the tree has no layout field: 0 non-zero lines), the nice value as such (see H5 caveat), idle CPU (4.4 % debug build while animating, 1.0 % with no spinner).
- The measured process was `nice 19` (inherited from this Worker session) on a 16-core machine at load 7-18; the User's real tree view runs `nice 0`. Debug profile. So these numbers are pessimistic, yet clean: the tree's stutter, if it still shows for the User, is not reproduced here (unverified why; see Fix proposal for the open candidates).
- The User's report ("worse in the plan view") is `stutter-measure-plan`'s; this file covers the tree only.

## Method

1. `scripts/cargo.sh build -p cpd` (**debug** profile, not release); binary `target/debug/cpd` of this worktree, run by path.
2. `COPY=$(mktemp -d)`; `sqlite3 ~/.cpd/cpd.db ".backup $COPY/cpd.db"`; `mkdir $COPY/run $COPY/locks`; `cp ~/.cpd/config.toml $COPY/`. The copy is 119.6 MB, 1478 sessions, 43177 events, 1763 Inbox items, 5 running Jobs. Real `~/.cpd` never written.
3. Spinner: in the copy only (python3 `sqlite3`, with `create_function("cpd_txn", 0, ...)` because the history triggers call it) the Lead `62b1c19b-...` of Plan `quality` (the largest Plan of the copy: 213 Jobs, 50 PRs) was un-archived and set `state='working'`, so its dot spins. Scenario D: every `state <> 'stopped'` session of the copy set to `stopped` (no spinner anywhere).
4. Private tmux: `TMUX_TMPDIR=$(mktemp -d); unset TMUX; tmux -L anim-tree -f /dev/null new-session -d -x 200 -y 50 -e CPD_HOME=$COPY -e CPD_TUI_TRACE=1 'target/debug/cpd tui view tree'`; run 35-40 s (under the 45 s engine-down limit); `kill-server`; trace read from `$COPY/run/tui/tree.log` (one run per scenario, one trace file each, removed before each).
5. B: a python3 `sqlite3` connection did `BEGIN IMMEDIATE`, slept 3 s, `COMMIT`, slept 2 s, 5 times from t = 2 s (fault injection on the copy).
6. Numbers: parsed the `trace gap_us=.. read_us=.. layout_us=.. draw_us=..` lines (percentile = sorted index `int(p*n)`; the first line's `gap_us=0` is dropped from the gap stats; read/layout/draw stats use non-zero values only). CPU = (utime+stime from `/proc/<pid>/stat`) / wall over the window. Nice from `ps -o ni`.
7. `strace -c -p` was tried: `ptrace(PTRACE_SEIZE): Operation not permitted` in this sandbox, so no syscall counts (unverified).
8. Teardown: `tmux -L anim-tree kill-server`, `rm -rf` of `$COPY` and `TMUX_TMPDIR`.

## Results

Tree view, debug build, nice 19, load average 7.5-17.9 during the runs.

| Scenario | draws | window | gap min / p50 / p95 / max (ms) | gaps > 150 ms | gaps > 500 ms | CPU | nice |
|---|---|---|---|---|---|---|---|
| A idle, spinner | 367 | 39 s | 102.9 / 105.8 / 115.4 / 124.1 | 0 (0 %) | 0 (0 %) | 4.45 % | 19 |
| B store write-locked 5 x 3 s | 371 | 40 s | 102.5 / 106.8 / 132.6 / 175.4 | 6 (1.6 %) | 0 (0 %) | 4.4 % | 19 |
| D no spinner | 38 | 36.5 s | 616.5 / 1010.7 / 1063.4 / 1068.6 | 37 (100 %) | 37 (100 %) | 1.0 % | 19 |

D's gap of about 1 s is by design (the Inbox ages redraw once a second; no spinner needs more), not a stutter.

Non-zero fields (count of draw lines, p50 / p95 / max in ms):

| Scenario | read_us | layout_us | draw_us |
|---|---|---|---|
| A | 184 lines, 0.8 / 1.3 / 12.4 | 0 lines (never set by the tree) | 367 lines, 4.7 / 12.0 / 21.5 |
| B | 186 lines, 0.9 / 1.3 / 11.1 | 0 lines | 371 lines, 5.1 / 26.2 / 74.1 |
| D | 37 lines, 4.9 / 30.4 / 91.7 | 0 lines | 38 lines, 11.9 / 46.1 / 102.3 |

Reads: A and B: 184 and 186 lines with `read_us` > 0 in about 39-40 s, about 4.7 per second (every other draw, the 200 ms `events.poll_ms`). In D reads are summed into the next (1 s later) draw, so only "37 lines" is known; the per-second count there is unverified (expected 5/s from the code).
Top gaps B (gap / read / draw ms): 175/0/24, 160/0/25, 155/0/39, 154/1/5, 153/0/30, 151/0/10. In A: 124/0/5, 123/0/4, 121/0/11, 120/0/14, 119/0/16, 118/12/4.

## Cause

- **H1 (plan view only)**: not applicable to the tree. The tree's `View::animated()` (tui/src/tree/view.rs) counts `Dot::Working | Dot::Fixing` of non-archived Sessions plus merging Inbox items, i.e. the Lead dot does make it redraw: gap p50 105.8 ms in A.
- **H2 (reads on the draw thread block the draw)**: refuted for the tree under a busy store. Reads are on the draw thread, but 1.3 ms p95; with a 3 s write transaction held 5 times (B) the read max was 11.1 ms (A: 12.4 ms), no gap over 500 ms, and B's tail gaps (150-175 ms) have `read_us` 0-1 ms. WAL readers do not wait for a writer, so `store_busy_ms` (5000) never ran. **Unverified**: a read that needs an exclusive lock (WAL checkpoint/recovery, a writer long enough to be a checkpoint blocker) or a real heavily loaded store.
- **H3 (re-layout per frame)**: not applicable; the tree writes no `layout_us` (0 non-zero lines in A, B, D).
- **H4 (loop sleeps, wall-clock frame skips)**: partly confirmed as a mechanism, but not a stutter on the tree. The loop is `poll(100 ms)` + work, so the period is 100 ms + draw + reads: p50 105.8 ms, i.e. about 6 ms of work per loop. A period of 100 ms + work means the clock `frame = (unix ms / 100) % 8` advances by 1 on most draws but by 2 now and then (a skipped moon phase): gap p95 115 ms and p99 above that; a frame is skipped whenever a loop takes more than 100 ms beyond its draw's phase. Not counted here (unverified): the share of frame deltas of 2, readable from the `frame=` field of the same trace. No freeze (max 124 ms in A).
- **H5 (niced or starved)**: the measured process was `nice 19` at load 7.5-17.9 (inherited from the Worker session; unprivileged users cannot lower it), the real tree view is `nice 0`. Even so p95 stayed 115 ms (A) and 133 ms (B): starvation widened the tail (draw p95 12 ms to 26 ms in B) but did not freeze. Machine load as a cause is **not refuted for the User's view** (it competes with 5 Workers, builds and the engine; unverified).
- **Not thought of beforehand**: `draw_us` is the only field with a heavy tail (max 74 ms in B, 102 ms in D) with a debug build; a release build would be faster (unverified). D draws cost 11.9 ms p50 because they redraw the whole 1763-item Inbox. The first thing to check on the User's machine is the same trace there, `gap_us` in a plan view and a tree view.

## Fix proposal

The tree needs no fix for the measured cause: it has none (0 freezes, 0 gaps over 500 ms). Proposals, ordered, for the Lead to judge with `stutter-measure-plan`'s numbers:

1. Keep the tree's rule (`animated()`) as the model; the plan view's `Centre::spins()` must count the same spinners (Lead dot, Job pills) so the loaders redraw every 100 ms.
2. Move the reads (`snapshot`, `poll_changes`, `open_pulls`, `engine_running`, `marker::live`, `read::tree`) to a thread that sends a snapshot over a channel, so the draw thread only draws and reads keys: at 12 ms max this is not needed for the tree today, but the store (119.6 MB, growing) and a checkpoint would make it so. Cheap to defer until a measure shows it.
3. Make `frame` advance by the draw count rather than the wall clock only if a skipped frame is a problem: with a 100 ms loop it skips about once in 8 draws (unverified; count the `frame=` deltas).
4. Take `draw_us` (p95 12-26 ms, Inbox of 1763 items) out of the tail: draw only the visible Inbox rows (unverified that it is not already so).

## Audit candidates

| Path | Number | Verdict |
|---|---|---|
| Idle CPU, spinner (A, debug, nice 19) | 4.45 % | fine (release is lower; unverified) |
| Idle CPU, no spinner (D) | 1.0 % | fine |
| Reads per second (A, B) | about 4.7 lines/s with `read_us` > 0, 0.8 ms p50, 12.4 ms max | fine |
| Reads in D | 37 lines in 36.5 s; p95 30.4 ms, max 91.7 ms (summed over about 5 polls) | fine, watch: 6x the A cost because the sum covers 5 polls |
| Draw, spinner (A) | 4.7 ms p50, 12.0 p95, 21.5 max | fine |
| Draw under load (B) | 5.1 p50, 26.2 p95, 74.1 max | slow tail (the one field with one); check on release |
| Draw, no spinner (D) | 11.9 ms p50, 46.1 p95, 102.3 max (1 s cadence) | slow-ish per draw, fine per second |
| Re-layout per frame | the tree has no layout field | n/a |
| Store busy (B) | 5 x 3 s write lock: reads unaffected | fine (WAL) |
| `strace` syscall counts | not available (ptrace denied) | unverified |
