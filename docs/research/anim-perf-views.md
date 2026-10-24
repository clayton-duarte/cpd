# Other cockpit views: stutter survey and measurements (Job stutter-measure-views, attempt 1)

## Summary

- **widgets** (Deploy column): **does not stutter** in the measured setup. A (4 PR dots) and M (merge spinner only): 375 draws in 35 s, gap p50 101 ms, p95 103-105 ms, 0 above 150 ms, 6 of 381 frames skipped (1.6 %; the plan view skipped 15-22 %), CPU 0.7 %. B (store write-locked): 1 gap above 150 ms (188 ms), 0 above 500 ms.
- **widgets, a stale spinner (new, not in the hypotheses):** the Lead's `working` state is read only when a `KINDS` Event or a `pull_requests` change arrives; `session-state` is not in `KINDS`. Measured: Lead set `working` with a `session-state` Event: no spinner after 11 s, 3 draws (the 2 s sampler), 0 spinner frames. In the real store the `KINDS` Events come about 1.3 per minute, so the dot lags by up to about a minute and, I infer (unrun), may keep spinning after the Lead stopped.
- **worker**: no spinner; idle 1 draw in 30 s, CPU 0.1 %. While its file grows (1 s) each change re-reads and re-renders the whole 1.2 MB file on the drawing thread: read p50 11.5 ms, p95 20 ms, max 37 ms, draw p50 3.5 ms. Fine at this size.
- **logs**: no spinner; idle 1 draw, CPU 0.1 %. With 5 Events/s it draws every 200 ms (p50 204.6 ms), read p50 0.34 ms, CPU 0.43 %. Fine.
- **history, chat, dialog, legend, merge popup**: no spinner, no read in a loop (blocking `event::read`). Not measured: nothing to measure.

## Survey

On screen in the cockpit: `tree`, `plan`, `widgets` (panes; tree and plan measured by other Jobs). On demand: `worker` (beside the chat), `history` and `chat` (chat slot), `logs`, `legend`, dialogs and the merge popup (tmux popups).

| View | Spinner | What redraws it | Reads, how often, which thread |
|---|---|---|---|
| tree, plan | yes | see anim-perf-tree.md, anim-perf-plan.md | those documents |
| widgets | yes: `lead_working` dot, merging+clean PR's merge cell, busy menu entry | `redraw` flag (a read changed `prs`, a key/click/wheel, the error, the 2 s CPU/GPU/mem sample), or a new `palette::frame` while `spins(prs)`; `event::poll(tick)` with `tick` = `FRAME` 100 ms when spinning, `TICK` 200 ms else | every `COALESCE` 500 ms (the loop's draw thread): `poll_changes` (Event query plus `pulls_mark`), `open_pulls` (+ `open_prs` when something moved) and the merge markers (`read_dir` plus pid check); `/proc` every 2 s; one thread |
| worker | none | `redraw`: input, or the check saw a change; `poll(TICK 100 ms)` | every `CHECK` 500 ms: `follow.poll`; when an Event came `read::worker`; the session file when its size/mtime changed (`refresh`, whole file); draw thread |
| logs | none | Events found, error, input; `poll(events.poll_ms)` (200 ms) | `follow.poll` each loop; draw thread |
| history | none | key, mouse, resize (blocking `event::read`) | none after start |
| chat | none | any event (blocking `read`); one static line | none |
| dialog, legend | none | key/mouse (blocking `read`) | none |
| menu (in widgets) | yes: busy `Merging` entry via `draw_at(frame)` | only the widgets loop | none |

Only `widgets` is both animated and reading in its loop; `worker` and `logs` read in their loops without animation.

## Method

Differences from anim-perf-plan.md (release build, same trace format):

1. Throwaway patch, never committed (`git checkout -- tui/` before the commit): `Trace` in `widgets::show` (draw, the read block, `marker::live`, the sampler), `worker::event_loop` (the check block, draw) and `logs::show` (`follow.poll`, draw). Idle `read_us` appears only on a draw line, so reads of idle views are not counted: unverified.
2. COPY by `sqlite3.Connection.backup`; only `config.toml` and empty `run/`, `locks/` (not the real `run/`); triggers dropped. Private tmux server `anim-views` (200x50, `CPD_TUI_TRACE=1`), a run 30 s (worker, logs) or 35 s (widgets), killed at the end; `TMUX_TMPDIR` and COPY are `mktemp` dirs, removed at the end.
3. widgets: the 4 `authored` open pull requests of the store have no Plan, so 4 `prs` rows of `quality` (merged) were pointed at them in the COPY. A = Lead `working`; M = Lead `stopped`, PR 2309 `clean` plus a merge marker with a live pid (merge spinner only); B = A with `BEGIN IMMEDIATE` held 5 x 3 s; D = Lead `stopped`, no marker. Screen checked with `capture-pane`.
4. worker: Attempt `delete-core-a1` (Plan `hygiene`), a copy of its 1.2 MB session file in the COPY. A = idle (also D: no spinner), B = A with the store locked, E = a copy of the last 3 lines appended each second. logs: A, B, E = 5 Events/s inserted (a copy of the last Event).
5. Machine: load average 4-30 (other Jobs), the views at nice 19 (inherited; the real views run at nice 0, see anim-perf-plan.md H5), so the numbers are pessimistic. No `strace`. Reads per second: `/proc/<pid>/io` syscr over the run.

## Results

Gaps in ms (all draws but the first), CPU % of one core over the run.

| View, scenario | draws | gap min / p50 / p95 / max | > 150 ms | > 500 ms | frames skipped | CPU % | ni |
|---|---|---|---|---|---|---|---|
| widgets A (4 Lead dots) | 376 | 100.4 / 101.1 / 103.4 / 119.8 | 0 | 0 | 6 of 381 | 0.74 | 19 |
| widgets M (merge spinner only) | 375 | 100.4 / 101.2 / 104.7 / 120.6 | 0 | 0 | 6 | 0.74 | 19 |
| widgets B (A + store locked) | 375 | 100.4 / 101.1 / 103.0 / 187.6 | 1 (0.3 %) | 0 | 6 | 0.71 | 19 |
| widgets D (no spinner) | 19 in 35 s | 2010 / 2012 / 2036 / 2036 (the 2 s sample) | - | - | n/a | 0.11 | 19 |
| worker A, B (idle, no spinner) | 1 in 30 s | - | - | - | n/a | 0.10 | 19 |
| worker E (file grows 1/s) | 31 | 535 / 1028 / 1059 / 3579 (the writer's 1 s) | n/a | n/a | n/a | 0.77 | 19 |
| logs A, B (idle) | 1 in 30 s | - | - | - | n/a | 0.10-0.13 | 19 |
| logs E (5 Events/s) | 106 | 201 / 205 / 412 / 3224 | n/a | 5 | n/a | 0.43 | 19 |

| View, scenario | read_us (non-zero): n, p50 / p95 / max ms | draw_us: n, p50 / p95 / max ms | syscr/s | syscw/s |
|---|---|---|---|---|
| widgets A | 75: 0.52 / 0.81 / 15.4 | 375: 0.56 / 0.84 / 4.6 | 9.5 | 49 |
| widgets M | 74: 0.57 / 0.82 / 15.6 | 374: 0.56 / 1.55 / 7.3 | 13.5 | 49 |
| widgets B | 90: 0.45 / 0.61 / 19.4 | 374: 0.56 / 0.84 / 5.2 | 9.3 | 49 |
| widgets D | 18: 1.74 / 21.3 / 21.3 | 18: 0.55 / 0.71 / 0.71 | 9.3 | 2.4 |
| worker E | 30: 11.5 / 20.1 / 36.6 | 30: 3.5 / 20.2 / 20.8 | 2.0 | 6.8 |
| logs E | 105: 0.34 / 1.05 / 4.0 | 105: 0.90 / 4.1 / 6.9 | 14.2 | 22 |

"n/a": gaps follow the input, not a clock, so skipped frames and a 150 ms share mean nothing. Widgets frames: 8 distinct, advance by 1: 369, by 2: 6 (A). Widgets D redraws every 2.0 s only for the CPU/GPU/mem sample. The logs E gaps above 500 ms (5, max 3.2 s) coincide with the load (25-30) and my insert script: unverified which.

Hypotheses:

- **H1 (widgets `spins(prs)` freezes a spinner drawn for another reason): refuted for the drawn spinners.** The row spinners are exactly `lead_working` and `merging && clean` (`column.rs` 175, 204), the same as `spins()`: A and M both 375 draws, 8 frames. **But the Lead dot itself is stale**: a `session-state` Event is not in `KINDS` (0 spinner frames in 11 s after the Lead went `working`). The menu's busy entry is drawn from the row's snapshot: unverified (needs a click).
- **H2 (fixed `poll(FRAME or TICK)` skips phases): confirmed, small.** p50 gap 101 ms, 6 of 381 frames advance by two (1.6 %), against 15-22 % in the plan view; the cause is a read block (max 15-19 ms) or a loaded machine, not the draw (p50 0.56 ms).
- **H3 (it reads the pull request rows every `TICK` on the drawing thread): refuted for the rate, confirmed for the thread.** The read block runs every `COALESCE` 500 ms (75 non-zero reads in 35 s = 2.1/s), p50 0.5 ms, max 15-19 ms (the first `pulls` change); CPU 0.74 %. The 5 s lock (B) did not delay it (max 19.4 ms).
- **H4 (the worker's 100 ms tick and 500 ms check read a log file on the drawing thread): partly confirmed.** The 100 ms tick reads nothing; the check reads the store and stats the file, and reads the file only when its size or mtime changed: idle CPU 0.10 %, no draws. When the file changes (E) the whole 1.2 MB is re-read and re-rendered on the drawing thread: 11.5 ms p50, 36.6 ms max. The idle cost of the 500 ms check is unverified (no draw line, no `strace`).

## Audit candidates

| Hot path | Measured | Verdict |
|---|---|---|
| Widgets redraw rule for the Lead dot (`session-state` not in `KINDS`) | 0 spinner frames in 11 s after the Lead started; real `KINDS` rate about 1.3/min | **slow (broken)**: add `Kind::SessionState` to `KINDS` (or read `lead_working` on each `COALESCE`); a test per spinner source |
| Widgets `poll(FRAME)` after the draw | 1.6 % frames skipped, p95 gap 103 ms | fine |
| Widgets read block every 500 ms | p50 0.5 ms, max 19 ms, 2.1/s | fine |
| Widgets draw | p50 0.56 ms | fine |
| Widgets idle CPU, spinner / none | 0.74 % / 0.11 % | fine |
| Widgets with a busy store (5 x 3 s) | 1 gap of 188 ms, none above 500 ms | fine |
| Widgets 2 s sample redraw (D) | 0.11 % CPU | fine |
| Worker re-read and re-render of the session file | 11.5 ms p50, 36.6 ms max at 1.2 MB, per change | fine at this size; unverified for 10 MB+ (propose appending only the new lines if it is slow) |
| Worker idle, 100 ms tick / 500 ms check | 0.10 % CPU | fine |
| Logs `follow.poll` (200 ms) | p50 0.34 ms, 0.43 % CPU at 5 Events/s | fine |
| Logs, history, chat, dialog, legend | no spinner; idle 0.10-0.13 % | fine |
| Merge popup, menu busy entry | not run (needs a click) | unverified |
| Idle read cost of worker and logs | no draw line | unverified |
