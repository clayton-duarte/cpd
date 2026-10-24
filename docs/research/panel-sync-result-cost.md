# Panel sync: idle cost and write load, before and after

Job `final-cost2`, Attempt 1, Plan `sync` (decision hs-d595 part 3). Findings only; no design. Taken 2026-10-24
10:41 to 11:20 EDT on `hs` (16 cores, `getconf CLK_TCK` = 100). A/B of two release builds on one machine in one
sitting, from throwaway homes. Every number says its sample count `n` (windows or runs). `verified` says how;
`unverified` and `guess:` mark the rest.

## Summary

**Reported first: numbers worse than BEFORE** (the rule: AFTER median minus BEFORE median is above the larger spread
of the 3 windows of either side; CPU resolution is 0.0083 % = one 10 ms tick in 120 s).

1. **Under the write load (244 commits per minute, 60 s, n=3 windows per side) every view and the engine costs more
   than BEFORE.** Tree 0.40 to 0.87 % CPU (x2.2), plan 0.30 to 0.82 % (x2.7), Dispatch (widgets) 0.07 to 0.42 %
   (x6), engine own 0.17 to 0.97 % (x5.8) and its children 0.68 to 3.05 % (x4.5). Wakeups per second of a view go from
   about 15 (widgets 6) to about 100. Causes, each measured (section 4): (a) the views read once per commit
   (4.07 reads/s, n=3 strace runs) where BEFORE coalesced to 1.3 to 1.8 reads/s, so the reader thread
   (0.30 to 0.62 % in the tree) and the draws (4.07/s against 1.3 to 1.8/s) double; (b) a new file-watcher thread
   (`notify-rs inoti`) wakes about 62 times a second under 4.07 commits/s (about 15 per commit; per-commit count
   `guess:` from the ratio) at 0.07 to 0.08 % CPU; (c) the engine ticks 2.18 times a second against 0.40, and a tick
   costs the same (4.2 to 4.4 ms of own CPU, 14 to 17 ms in children, in both builds), so its cost scales with the
   tick count.
2. **Idle, threads and memory are above BEFORE:** every view has 2 (tree, plan) or 3 (widgets) more threads (the
   watcher and the input thread, and for widgets also a reader), and the widgets view has +1.75 MiB resident
   (13.5 to 15.25 MiB, spread 0.16); tree +0.20 MiB (inside the spread) and plan +0.30 MiB (0.05 above the spread), the engine +0.38 MiB (inside
   the spread). The engine's own idle CPU is +0.042 percentage points (0.133 to 0.175 %, 5 ticks of 10 ms in
   120 s; spread 0.017); cause unverified.
3. **Idle CPU and wakeups of the views are far below BEFORE:** tree 0.167 to 0.017 % and 14.1 to 3.6 wakeups/s,
   plan 0.108 to 0.017 % and 14.1 to 3.6, widgets 0.067 to 0.033 % and 6.0 to 5.1 (n=3 windows of 120 s).
4. **The engine: at most 2 ticks per second does not hold under a sustained load.** n=3 runs of 60 s: 131 tick
   starts per run = 2.18/s, and 3 starts inside one sliding second in all 3 runs. In the 100-commit burst it holds
   (at most 2 in any second, n=3). Cause of the excess: every 5 s a second tick starts 41 to 52 ms after the
   previous one, in BEFORE too (BEFORE idle: 0.40/s, n=3); 12 such pairs per minute on top of the 119 ticks 0.5 s
   apart. The reason for the pair is `guess:` E5's due time; not verified.
5. The burst of 100 commits in 0.98 to 0.99 s makes at most 1 snapshot per view per 100 ms frame (10 in the burst
   second, 11 over -1 to +3 s, n=3 runs, all three views): holds. The engine never stopped ticking while all three
   views were stopped with SIGSTOP for 20 s (44 tick starts in AFTER, the rate of the live-view runs; n=3): holds.

Latency from commit to drawn (promise 1) is not this file's scope: it is `panel-sync-result-latency.md`
(Job `final-docs`). Nothing here measures it.

### Summary table

Median (min to max) of n=3 windows per side, BEFORE `4a071cf5` against AFTER `d5bf3c42`. CPU is utime + stime of
the process as a percentage of one core. "Wake/s" is the sum of voluntary and involuntary context switches of all
threads (`/proc/<pid>/task/*/status`). MiB is VmRSS at the end of the window. Verdict: `above` when AFTER minus
BEFORE is positive and larger than both spreads (threads: any increase), `below` the mirror, else `within`.

| Row | Metric | BEFORE | AFTER | Difference | Verdict against "no run-time cost above today" |
|---|---|---|---|---|---|
| tree, idle | CPU % | 0.167 (0.158 to 0.167) | 0.017 (0.017 to 0.025) | -0.150 | below |
| tree, idle | wake/s | 14.1 (14.0 to 14.2) | 3.63 (3.57 to 3.65) | -10.5 | below |
| tree, idle | MiB / threads | 14.74 / 2 | 14.94 / 4 | +0.20 / +2 | MiB within; **threads above** |
| plan, idle | CPU % | 0.108 (0.108 to 0.108) | 0.017 (0.017 to 0.017) | -0.092 | below |
| plan, idle | wake/s | 14.1 (14.1 to 14.1) | 3.62 (3.62 to 3.66) | -10.5 | below |
| plan, idle | MiB / threads | 16.12 / 2 | 16.42 / 4 | +0.30 / +2 | **above** (spread 0.25) / **above** |
| widgets, idle | CPU % | 0.067 (0.067 to 0.067) | 0.033 (0.033 to 0.042) | -0.033 | below |
| widgets, idle | wake/s | 5.98 (5.98 to 5.98) | 5.13 (5.11 to 5.19) | -0.85 | below |
| widgets, idle | MiB / threads | 13.50 / 2 | 15.25 / 5 | +1.75 / +3 | **above** / **above** |
| engine, idle | CPU % (own) | 0.133 (0.125 to 0.142) | 0.175 (0.167 to 0.175) | +0.042 | **above** (spread 0.017) |
| engine, idle | children CPU % | 0.333 (0.325 to 0.333) | 0.342 (0.333 to 0.342) | +0.008 | within |
| engine, idle | wake/s | 11.7 (11.7 to 11.7) | 10.9 (10.9 to 10.9) | -0.88 | below |
| engine, idle | MiB / threads | 12.96 / 1 | 13.33 / 1 | +0.38 / 0 | within |
| tree, load | CPU % | 0.400 (0.383 to 0.433) | 0.867 (0.850 to 0.867) | +0.467 | **above** |
| tree, load | wake/s | 15.6 (15.6 to 15.8) | 103.0 (95.6 to 104.0) | +87.4 | **above** |
| tree, load | MiB / threads | 14.68 / 2 | 14.97 / 4 | +0.29 / +2 | **above** / **above** |
| plan, load | CPU % | 0.300 (0.283 to 0.300) | 0.817 (0.783 to 0.833) | +0.517 | **above** |
| plan, load | wake/s | 15.3 (15.1 to 15.3) | 98.1 (95.1 to 100.5) | +82.9 | **above** |
| plan, load | MiB / threads | 16.09 / 2 | 16.25 / 4 | +0.16 / +2 | MiB within; **threads above** |
| widgets, load | CPU % | 0.067 (0.067 to 0.083) | 0.417 (0.383 to 0.417) | +0.350 | **above** |
| widgets, load | wake/s | 5.98 (5.98 to 6.03) | 104.8 (102.0 to 108.5) | +98.8 | **above** |
| widgets, load | MiB / threads | 13.35 / 2 | 15.14 / 5 | +1.79 / +3 | **above** / **above** |
| engine, load | CPU % (own) | 0.167 (0.167 to 0.183) | 0.967 (0.967 to 1.000) | +0.800 | **above** |
| engine, load | children CPU % | 0.683 (0.667 to 0.717) | 3.050 (3.033 to 3.100) | +2.367 | **above** |
| engine, load | wake/s | 12.0 (12.0 to 12.0) | 16.4 (16.4 to 16.7) | +4.45 | **above** |
| engine, load | MiB / threads | 13.01 / 1 | 13.52 / 1 (one window 2) | +0.51 / 0 | **above** (MiB) |
| engine, load | tick starts per s | 0.40 (n=3) | 2.18 (n=3) | +1.78 | **above**; max 3 in a second |
| tree, load | snapshots per s (strace) | 1.77 (1.77 to 1.80) | 4.07 (4.05 to 4.07) | +2.30 | above |
| plan, load | snapshots per s (strace) | 1.35 (1.33 to 1.38) | 4.07 (4.07 to 4.07) | +2.72 | above |
| widgets, load | read bursts per s (strace) | 1.65 (1.65 to 1.65) | 4.10 (4.08 to 4.10) | +2.45 | above |

Windows: idle 120 s, load 60 s, n=3 each per side (round 1 to 3, BEFORE then AFTER in every round).

## 1. Method

- Builds. BEFORE `4a071cf5` (`git rev-parse b9b5338b^1`, verified). AFTER `d5bf3c42`, the tip of origin/main when
  I started (verified: `git log --first-parent origin/main` shows the merges of PR sync `b9b5338b` #307, engine-tick
  `fea4e885` #310, facts `7cfd48ac` #309 and views `bd179cbd` #315). `cpd --version` of both: `0.13.45-dev`
  (so the build tells them apart only by path). Release builds with `scripts/cargo.sh build --release -p cpd`, each in
  its own `git worktree` (`/tmp/sync-probe-cost2/{before,after}`) and `CARGO_TARGET_DIR`, sccache on.
- Merges between the two that are **not** this Plan's, all in AFTER and not separable from the Plan's effect (their
  effect on cost: `guess: small`, none touches the views' loops or the engine's tick; checked only from the titles
  and `git diff --stat`, not read): `b9cb782c` #308 box widget, `d209150d` #297 Context owner, `01e7db47` #312
  (a test deleted), `feacda92` #313 policy scopes, `439a37e8` #314 fake tmux for tests, `d5bf3c42` #317 yellow Job.
- A cockpit per window: `mktemp -d /tmp/sync-probe-cost2-home.XXXX` as `CPD_HOME`, `config.toml` naming its own tmux
  socket, a private server `tmux -L sync-probe-cost2-<pid>-<n> -f /dev/null`, the seed of `panel-sync-latency.md`
  (Context `ctx`, Plan `pay` in `deploy`, its Lead `lead1` **idle** instead of working so the tree does not spin,
  one PR, one draft Job; same SQL for both), the three `cpd tui view <v>` and `cpd watch`, each under
  `exec timeout 900`. A stub `gh` (a `sh` script answering the GraphQL calls with an empty valid search) in `PATH`;
  `pr_poll_seconds` is the default (5), so E5 polls every 5 s in both builds (verified: 6 `cpd pr poll` execs in 30 s
  in both, section 5). 15 s settle, then the window. The harness cleans `TMUX`, `CPD_*` and `PI_*` from the environment
  and never touches `~/.cpd` or the `cpd` socket (every tmux call is `-L sync-probe-cost2-...`, asserted in code).
- Idle window: 120 s, no trace, nothing else attached. Load window: 60 s, `CPD_TUI_TRACE=1` on both sides (it appends a
  line per drawn frame; same on both), and a loop of real `cpd context note --context ctx` commands (the binary under
  test) at 244 per minute on an absolute schedule: the busiest ordinary minute of `panel-sync-cost.md` section 3
  (244 commits; the 1 121 of that section is a bulk archive and is replayed as the burst below). 244 of 244 commands
  succeeded in all 6 windows; each wall time p50 10.5 to 11.2 ms.
- Measures: `/proc/<pid>/stat` utime + stime (CPU), cutime + cstime (children waited for), `/proc/<pid>/task/*/status`
  voluntary + nonvoluntary context switches (wakeups; reported as the sum over threads and as the main thread alone,
  because the baseline's 10.2/s of `panel-sync-cost.md` is the main thread alone), `VmRSS` at both ends, `Threads`.
  Engine tick starts: distinct values of `st_mtime_ns` of `locks/watch.lock`, polled every 20 ms (only
  `lock.touch()` sets it, at the start of each tick: verified by `grep` of `engine/src`).
- Snapshot counts (store reads) per view: `strace -f -e trace=pread64` wrapped around the view, in separate runs (not
  the CPU windows, strace slows the view). A snapshot is a burst of at least 15 `pread64` of one thread with gaps of at
  most 3 ms (widgets: at least 3, its reads are smaller). Verified as a detector: in the 3 AFTER load runs it counted
  243, 244, 244 (tree) and 244 (plan) for 244 commits, as one read per commit; whether the widgets' small bursts are a
  snapshot or the poll is unverified.
- Burst: 100 SQL commits (one `context-record` Event each, in-process `sqlite3` writes in the way `Store::write`
  does) at 9.9 ms spacing, 0.98 to 0.99 s, with 3 s before and 6 s after, under strace on the views.
- Block test: `kill -STOP` of the three views, 20 s of the 244/min commits, `kill -CONT`.
- Interleaving: round r = idle(BEFORE), idle(AFTER), load(B), load(A), loadread(B), (A), burst, stop; 3 rounds.
  Each run starts a fresh cockpit. Not literally A B A B per window length; BEFORE always first in a round.
- Beside it: `/proc/loadavg` and the machine-wide busy share (from `/proc/stat`) of every window are in section 2. At
  the start of the Job the machine had a load average above 20 (my two builds and other agents' Jobs, seen with
  `ps`); the idle and load windows ran at a load average of 0.3 to 1.7 (1 min). What ran beside the windows (other
  agent sessions, `k3s-server`) was not captured per window (unverified beyond the busy share).

## 2. Idle windows (120 s, n=3 per side)

Per process: median and range (min to max) of 3 windows.

| Process | Build | CPU % | Children CPU % | Wake/s, all threads | Wake/s, main thread | RSS MiB start / end | Threads |
|---|---|---|---|---|---|---|---|
| tree | BEFORE | 0.167 (0.158 to 0.167) | 0 | 14.1 (14.0 to 14.2) | 10.00 | 14.7 / 14.7 | 2 |
| tree | AFTER | 0.017 (0.017 to 0.025) | 0 | 3.63 (3.57 to 3.65) | 1.00 | 14.9 / 14.9 | 4 |
| plan | BEFORE | 0.108 (0.108 to 0.108) | 0 | 14.1 (14.1 to 14.1) | 10.00 | 16.1 / 16.1 | 2 |
| plan | AFTER | 0.017 (0.017 to 0.017) | 0 | 3.62 (3.62 to 3.66) | 1.00 | 16.4 / 16.4 | 4 |
| widgets | BEFORE | 0.067 (0.067 to 0.067) | 0 | 5.98 (5.98 to 5.98) | 4.98 | 13.5 / 13.5 | 2 |
| widgets | AFTER | 0.033 (0.033 to 0.042) | 0 | 5.13 (5.11 to 5.19) | 1.50 | 15.2 / 15.2 | 5 |
| engine | BEFORE | 0.133 (0.125 to 0.142) | 0.333 (0.325 to 0.333) | 11.7 (11.7 to 11.7) | 11.7 | 13.0 / 13.0 | 1 |
| engine | AFTER | 0.175 (0.167 to 0.175) | 0.342 (0.333 to 0.342) | 10.9 (10.9 to 10.9) | 10.9 | 13.3 / 13.3 | 1 |

Per thread (median of 3 windows; wake/s, CPU %):

- tree BEFORE: `cpd` 10.00/s 0.033 %; `cpd-reads` 4.08/s 0.133 %. AFTER: `cpd` 1.00/s 0.008 %; `cpd-input` 0.00/s;
  `cpd-reads` 0.50/s 0.017 %; `notify-rs inoti` 2.13/s 0.000 %.
- plan BEFORE: `cpd` 10.00/s 0.025 %; `cpd-reads` 4.11/s 0.083 %. AFTER: `cpd` 1.00/s 0 %; `cpd-input` 0.00/s;
  `cpd-reads` 0.50/s 0.008 %; `notify-rs inoti` 2.12/s 0 %.
- widgets BEFORE: `cpd` 5.97/s 0.067 %. AFTER: `cpd` 2.50/s 0.017 %; `cpd-input` 0.00/s; `cpd-reads` 0.50/s
  0.017 %; `notify-rs inoti` 2.12/s 0 %.
- engine: one thread in both (BEFORE 11.74/s 0.117 %, AFTER 10.87/s 0.158 %).

Reproduction of the baseline of `panel-sync-cost.md`: BEFORE's main thread wakes 10.0/s (tree, plan) and 5.0/s
(widgets): the baseline's 10.2, 10.0 and 5.0 (verified: same numbers). CPU is lower than the live baseline (tree
0.37 %, plan 0.36 %, widgets 0.14 %, engine 0.50 %) and the engine wakes 11.7/s against 63/s: the throwaway store holds
one Plan and one Lead where the live one holds hundreds of rows and several Workers (guess: that explains the CPU).
Engine ticks: 48 starts in 120 s in every window of both builds (0.40/s): a pair 41 to 52 ms apart every 5 s
(5.04 s between pairs). Engine tick starts per window:

| Window | loadavg 1 / 5 / 15 min at start | Machine busy % | Engine tick starts |
|---|---|---|---|
| r1 BEFORE | 1.03 7.91 13.86 | 3.6 | 48 |
| r1 AFTER | 0.25 5.10 12.01 | 4.6 | 48 |
| r2 BEFORE | 1.51 1.74 6.94 | 7.4 | 48 |
| r2 AFTER | 1.38 1.55 6.16 | 7.8 | 48 |
| r3 BEFORE | 1.37 1.32 3.89 | 7.2 | 48 |
| r3 AFTER | 1.52 1.31 3.52 | 7.8 | 48 |

## 3. Load windows (244 commits in 60 s, n=3 per side)

| Process | Build | CPU % | Children CPU % | Wake/s, all threads | Wake/s, main thread | RSS MiB start / end | Threads |
|---|---|---|---|---|---|---|---|
| tree | BEFORE | 0.400 (0.383 to 0.433) | 0 | 15.6 (15.6 to 15.8) | 10.0 | 14.7 / 14.7 | 2 |
| tree | AFTER | 0.867 (0.850 to 0.867) | 0 | 103.0 (95.6 to 104.0) | 32.1 | 15.0 / 15.0 | 4 |
| plan | BEFORE | 0.300 (0.283 to 0.300) | 0 | 15.3 (15.1 to 15.3) | 10.0 | 16.1 / 16.1 | 2 |
| plan | AFTER | 0.817 (0.783 to 0.833) | 0 | 98.1 (95.1 to 100.5) | 27.3 | 16.2 / 16.2 | 4 |
| widgets | BEFORE | 0.067 (0.067 to 0.083) | 0 | 5.98 (5.98 to 6.03) | 4.98 | 13.4 / 13.4 | 2 |
| widgets | AFTER | 0.417 (0.383 to 0.417) | 0 | 104.8 (102.0 to 108.5) | 33.8 | 15.1 / 15.1 | 5 |
| engine | BEFORE | 0.167 (0.167 to 0.183) | 0.683 (0.667 to 0.717) | 12.0 (12.0 to 12.0) | 12.0 | 12.9 / 13.0 | 1 |
| engine | AFTER | 0.967 (0.967 to 1.000) | 3.050 (3.033 to 3.100) | 16.4 (16.4 to 16.7) | 16.4 | 13.3 / 13.5 | 1 (2 in one window) |

Per thread (median of 3 windows; wake/s, CPU %):

- tree BEFORE: `cpd` 10.02/s 0.067 %; `cpd-reads` 5.60/s 0.300 %. AFTER: `cpd` 32.13/s 0.133 %; `cpd-reads` 8.23/s
  0.617 %; `notify-rs inoti` 62.40/s 0.083 %; `cpd-input` 0.
- plan BEFORE: `cpd` 10.02/s 0.083 %; `cpd-reads` 5.25/s 0.200 %. AFTER: `cpd` 27.30/s 0.183 %; `cpd-reads` 8.23/s
  0.517 %; `notify-rs inoti` 62.51/s 0.083 %.
- widgets BEFORE: `cpd` 5.98/s 0.067 %. AFTER: `cpd` 34.81/s 0.067 %; `cpd-reads` 8.27/s 0.233 %;
  `notify-rs inoti` 61.76/s 0.083 %.
- engine BEFORE: 11.98/s 0.150 %. AFTER: 16.43/s 0.900 % (the sum of the thread's CPU; the process figure is 0.967).

Frame trace of the tree and the plan (`CPD_TUI_TRACE=1`, per 60 s window, n=3 per side):

| View | Build | Draws per s | Frames with a read per s | Gap p50 (ms) | `read_us` max (us) |
|---|---|---|---|---|---|
| tree | BEFORE | 1.75, 1.75, 1.77 | the same | 600 | 160 to 175 |
| tree | AFTER | 4.07 (all 3) | the same | 246 | 18 to 49 |
| plan | BEFORE | 1.37, 1.32, 1.38 | the same | 700 to 799 | 138 to 162 |
| plan | AFTER | 4.07 (all 3) | the same | 246 | 16 to 21 |

Gap max 14.7 to 15.5 s in every window (the quiet start before the commits), so a gap max says nothing here.
The loop thread's read time (`read_us`) never exceeds 175 us in any window: the loop never waits on a read (verified,
n=6 windows per side).

Windows context:

| Window | loadavg 1 / 5 / 15 min at start | Machine busy % | Commit wall time p50 / max (ms) |
|---|---|---|---|
| r1 BEFORE | 0.51 3.45 10.46 | 3.9 | 10.5 / 13.2 |
| r1 AFTER | 0.89 2.93 9.73 | 4.4 | 10.9 / 57.5 |
| r2 BEFORE | 0.90 1.32 5.45 | 7.4 | 10.6 / 31.4 |
| r2 AFTER | 1.63 1.42 5.13 | 12.5 | 11.2 / 40.3 |
| r3 BEFORE | 1.04 1.17 3.17 | 8.3 | 10.6 / 30.1 |
| r3 AFTER | 1.23 1.19 3.01 | 10.2 | 11.2 / 50.3 |

The commit loop (which also runs a `cpd` process per commit) is not in any observed process's counters.

## 4. Why the load costs more: reads, draws, watcher, ticks

Store reads (snapshots) per view, `strace` runs of 60 s with the same 244 commits (n=3 per side; the per-100 ms and
per-second maxima are over the whole run):

| View | Build | Snapshots per run | Per second | Max per 100 ms | Max per 1 s |
|---|---|---|---|---|---|
| tree | BEFORE | 108, 106, 106 | 1.80, 1.77, 1.77 | 1 | 2 |
| tree | AFTER | 243, 244, 244 | 4.05, 4.07, 4.07 | 1 | 5 |
| plan | BEFORE | 81, 83, 80 | 1.35, 1.38, 1.33 | 1 | 2 |
| plan | AFTER | 244, 244, 244 | 4.07 (3) | 1 | 5 |
| widgets (bursts of >= 3 `pread64`) | BEFORE | 99, 99, 99 | 1.65 (3) | 1 | 2 |
| widgets (same) | AFTER | 246, 245, 246 | 4.10, 4.08, 4.10 | 1 | 5 |

- AFTER reads once per commit (commits are 246 ms apart, more than a 100 ms frame); BEFORE coalesced by its 500 ms
  rule. Reads per second: x2.3 (tree), x3.0 (plan), x2.5 (widgets). Draws of the tree and the plan follow the reads
  (4.07/s against 1.75 and 1.35/s, section 3). This is the behaviour the Plan asked for (at most one read per frame),
  and it is what costs CPU on a store with this write rate.
- CPU per extra commit (difference of CPU over 4.07 commits/s): tree 1.15 ms, plan 1.27 ms, widgets 0.86 ms (arithmetic
  on section 3 medians: 0.467, 0.517 and 0.350 percentage points over 4.07 commits/s). The tree's reader thread alone: 0.617 % against 0.300 % (x2.1) for x2.3 reads.
- The watcher thread: 62 wakeups/s in each view for 4.07 commits/s, 2.13/s idle (n=3 each). Its CPU is 0.083 % in
  each view, a tenth of the extra. What the inotify events are (the WAL, the shm, the db files) is unverified.
- Engine ticks (distinct `watch.lock` mtimes, 20 ms polling): 131 per 60 s AFTER (2.18/s) against 24 BEFORE (0.40/s);
  131 starts per run in each of 3 runs: 119 spaced 0.500 to 0.571 s and 12 that start 41 to 52 ms after another
  (the same count every run). Max starts in one sliding second: 3 (AFTER, 3 of 3 runs), 2 (BEFORE, same pairs). Own CPU per tick:
  AFTER 0.967 % / 2.18 = 4.4 ms, BEFORE 0.167 % / 0.40 = 4.2 ms; children per tick: AFTER 3.05 % / 2.18 = 14 ms,
  BEFORE 0.683 % / 0.40 = 17 ms (arithmetic; the cost per tick did not change, the number of ticks did).
- What a tick runs (`strace -f -e trace=execve` on the engine, 30 s of the same commit loop, one run per side, n=1):

  | Exec of the engine's children | BEFORE (12 ticks in 30 s, 0.4/s; unverified count) | AFTER (65 ticks in 30 s, 2.18/s; unverified count) |
  |---|---|---|
  | `tmux -u -L <sock> ...` | 11 | 64 |
  | `cpd context distill ...` | 8 | 51 |
  | `git -C /nowhere ...` (the seed's fake repo path) | 8 | 51 |
  | `gh api graphql` (the stub; E5's poll) | 12 | 12 |
  | `cpd pr poll` (E5) | 6 | 6 |

  So E5 polls every 5 s in both builds (6 in 30 s), and the other steps run about once per tick: the extra children are
  per tick, not per poll. The `git` and `distill` execs are steps the seed (a Plan in `deploy` with a fake repo, a
  Context taking notes) provokes; a live engine has a different mix (unverified).
- `cpd context note` writes: wall time p50 10.5 to 10.6 ms BEFORE, 10.9 to 11.2 ms AFTER (n=3 windows of 244),
  max 13 to 31 ms BEFORE and 40 to 58 ms AFTER. The max is one slow commit of 244 (cause unverified: the writers
  compete with 5x more ticks and 4x more reads in AFTER; guess).

## 5. Burst of 100 commits in 1 s, and the engine against a stopped view

Burst (n=3 per side; strace on the views; `ticks` are `watch.lock` mtimes, burst second = the 1.0 s from the first commit):

| Build | Writes (p50 / max ms) | Snapshots per view in the burst second | Max per 100 ms | Max per sliding 1 s | Snapshots over -1 to +3 s | Engine tick starts in the burst second | Max in a sliding second | Starts in 10 s |
|---|---|---|---|---|---|---|---|---|
| BEFORE | 5.4 to 5.5 / 7.2 to 11.1 | tree 2, plan 1 to 2, widgets 1 | 1 | 2 | 2 to 3 | 0 | 2 | 4 |
| AFTER | 3.9 to 5.6 / 7.8 to 8.5 | 10 (tree, plan, widgets) | 1 | 10 | 11 | 2 | 2 | 6 |

AFTER: at most one snapshot per 100 ms per view (verified from the strace times, n=3 runs x 3 views) and at most
10 in any second: the promise (3) holds for the views. The engine started at most 2 ticks in any second of the
burst runs, n=3: holds there (and not in the sustained load, section 4).

Draws during the burst (trace lines of the whole run after the settle): tree 0 (the Event kind is not on its list), plan
2 to 3 BEFORE and 11 AFTER, in all runs.

Engine against stopped views (`kill -STOP` of the tree, plan and widgets for 20 s while 82 `cpd context note` ran at
244/min; n=3 per side):

| Build | Engine tick starts in 20.1 s | Commits ok | Commit wall p50 / max (ms) | Views alive after `-CONT` |
|---|---|---|---|---|
| BEFORE | 8, 8, 7 | 82 of 82 each | 10.5 to 10.7 / 11.9 to 16.1 | 3 of 3 each |
| AFTER | 44, 44, 44 | 82 of 82 each | 10.8 to 11.0 / 30.3 to 50.0 | 3 of 3 each |

A re-run of the AFTER and BEFORE block test with the tick times kept (n=1 per side): BEFORE 8 starts, min interval
41 ms, 1 in a sliding second at most 2; AFTER 44 starts, min 42 ms, at most 3 in a sliding second, over a span of
19.7 s. The AFTER rate (44 in 20.1 s = 2.19/s) equals the rate with live views (2.18/s), and BEFORE's (8 in 20.1 s,
0.40/s) its own: stopped views changed neither. Promise (4) holds for these tests: a stopped view did not slow the
engine or the writers (verified). A view that holds a long read transaction was not tested.

## 6. What was not measured

- Latency from commit to drawn (promise 1): out of scope of this Job; see `panel-sync-result-latency.md`.
- Real `gh` and GitHub: the stub answers in a few ms and prints an empty search; a real poll takes 1 to 4 s of wall
  time and costs children CPU (`panel-sync-cost.md` section 1: 1.7 to 8.2 %). The engine's child CPU here is
  mostly `tmux`, `git`, `cpd context distill` and the stub.
- A live-sized store: the throwaway store has one Plan, one Lead, one PR and a few hundred notes. The live store is
  100 MB and its snapshots cost 4.6 to 10.5 ms of reader CPU per commit (`panel-sync-cost.md` section 2); with 4.07
  reads per second AFTER instead of 1.4 to 1.8 the extra would be larger than measured here (guess: scales with the
  read cost).
- Input: no key was pressed; the `cpd-input` thread shows 0.00 wakeups/s in every window (idle cost of typing
  untested). The tree with a spinning Lead (a `working` Session) was not measured: the Lead was idle.
- Several windows are 120 s or 60 s only, n=3; no night or weekend windows. The spread is the range of 3 windows,
  not a confidence interval.
- The cause of the pair of ticks 41 to 52 ms apart (`guess:` E5's due time) and the cause of the engine's +0.042
  percentage points idle CPU are not found. The inotify events per commit (about 15) is a ratio, not a count.
- The per-window `ps` of what ran beside the windows was not taken; the machine-wide busy share is in sections 2 and 3.
- BEFORE and AFTER differ in the 6 non-Plan merges of section 1; their effect is `guess: small`, not measured.

## 7. Repeat it

Everything runs from `/tmp/sync-probe-cost2` and removes its own homes and sockets; nothing touches `~/.cpd` or the
tmux socket `cpd`.

```sh
W=/tmp/sync-probe-cost2; mkdir -p $W/bin
git worktree add --detach $W/before 4a071cf5; git worktree add --detach $W/after d5bf3c42
for v in before after; do (cd $W/$v && CARGO_TARGET_DIR=$W/tgt-$v timeout 1500 scripts/cargo.sh build --release -p cpd); done
# gh stub (E5's poll answers an empty search): see the appendix; save as $W/bin/gh, chmod +x
pgrep -af sync-probe                                   # none of an earlier run may be left
timeout 3300 python3 $W/cost.py $W/out 3 idle,load,loadread,burst,stop   # ~37 min; cost.py is in the appendix
python3 $W/analyze.py $W/out; python3 $W/summ.py       # tables of sections 2 to 5
timeout 200 python3 $W/execs.py                        # section 4's exec counts (engine under strace)
```

Appendix files (all under `/tmp/sync-probe-cost2`, removed at the end; their text is below).

## Appendix: harness

`$W/bin/gh` (stub, `chmod +x`):

```sh
#!/bin/sh
case "$*" in *graphql*) echo '{"data":{"viewer":{"login":"probe"},"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4990,"resetAt":"2026-12-01T00:00:00Z"}}}' ;; *) exit 0 ;; esac
```

`cost.py` as saved at the end (the 3-round run used the version before two additive edits, the `strace='engine'`
option used by `execs.py` and the `ticks_abs` list of the `stop` kind, which the re-run of `stop` and `execs.py` used; one cockpit per run, every home `mktemp -d`, every tmux call `-L sync-probe-cost2-...` asserted):

```python
#!/usr/bin/env python3
"""A/B cost harness for the findings of Job final-cost2 (throwaway homes, private tmux sockets only).
usage: cost.py OUTDIR ROUNDS KINDS   (KINDS: comma list of idle,load,burst,stop; env IDLE_W, LOAD_W seconds)
Every cockpit: its own mktemp home, its own tmux socket sync-probe-cost2-<pid>-<n>; killed in `finally`."""
import os, sys, time, json, signal, sqlite3, datetime, subprocess as sp, threading, tempfile, shutil, atexit, itertools, re

ROOT = '/tmp/sync-probe-cost2'
BIN = {'before': ROOT + '/tgt-before/release/cpd', 'after': ROOT + '/tgt-after/release/cpd'}
TCK = os.sysconf('SC_CLK_TCK')
OUT = sys.argv[1]; ROUNDS = int(sys.argv[2]); KINDS = sys.argv[3].split(',')
IDLE_W = float(os.environ.get('IDLE_W', 120)); LOAD_W = float(os.environ.get('LOAD_W', 60))
SETTLE = float(os.environ.get('SETTLE', 15))
RATE = 244 / 60.0   # commits per second: the busiest ordinary minute of panel-sync-cost.md section 3
os.makedirs(OUT, exist_ok=True)
CLEAN = {k: v for k, v in os.environ.items() if not (k.startswith('CPD_') or k.startswith('TMUX') or k.startswith('PI_'))}
CLEAN['PATH'] = ROOT + '/bin:' + CLEAN['PATH']
SOCKS, HOMES, N = [], [], itertools.count()


def log(*a):
    with open(OUT + '/progress.log', 'a') as f:
        print(time.strftime('%H:%M:%S'), *a, file=f, flush=True)


def tmux(sock, *a, **kw):
    assert sock.startswith('sync-probe-cost2-'), sock
    return sp.run(['tmux', '-L', sock, *a], capture_output=True, text=True, env=CLEAN, **kw)


def cleanup():
    for s in SOCKS:
        sp.run(['tmux', '-L', s, 'kill-server'], capture_output=True, env=CLEAN)
    for h in HOMES:
        sp.run(['pkill', '-f', h], capture_output=True)
        shutil.rmtree(h, ignore_errors=True)


atexit.register(cleanup)
for sg in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
    signal.signal(sg, lambda *_: sys.exit(1))

Z = '2026-10-23T00:00:00.000Z'
SEED = [
    f"INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx','Ctx','{Z}','probe')",
    f"INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES ('pay','ctx','Pay','deploy','lead1','{Z}','{Z}')",
    f"INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_at, created_at) VALUES ('lead1','lead','ctx','pay','Lead','/','O3','%0','idle','{Z}','{Z}')",
    f"INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, mergeable, merge_state, authored) VALUES (1,'o/repo',1,'https://github.com/o/repo/pull/1','Probe PR one',0,'open','none','pending','{Z}','{Z}','mergeable','blocked',1)",
    f"INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at) VALUES ('pay','repo','/nowhere','main','open','cpd/pay','/nowhere/lead',1,'https://github.com/o/repo/pull/1','{Z}',1,cpd_txn(),'{Z}','{Z}')",
    f"INSERT INTO jobs (plan, pr, key, title, status, appended, created_txn, created_at) VALUES ('pay','repo','d1','Draft job one','draft',0,cpd_txn(),'{Z}')",
    f"INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES (cpd_txn(),'2026-10-23T00:00:01.000Z','session-moved','ctx',NULL,'lead1','f','{{\"pane\":\"%0\",\"window\":\"@0\",\"place\":\"chat\"}}')",
]


def now_at():
    n = datetime.datetime.now(datetime.timezone.utc)
    return n.strftime('%Y-%m-%dT%H:%M:%S.') + '%03dZ' % (n.microsecond // 1000)


def write(home, stmts):
    assert home.startswith('/tmp/sync-probe-cost2-home.'), home
    c = sqlite3.connect(home + '/cpd.db', isolation_level=None, timeout=5)
    c.execute('PRAGMA foreign_keys=ON'); c.execute('PRAGMA synchronous=NORMAL'); cur = {}
    c.create_function('cpd_txn', 0, lambda: cur['id']); c.execute('BEGIN IMMEDIATE')
    cur['id'] = tid = c.execute('SELECT coalesce(max(id),0)+1 FROM txns').fetchone()[0]
    c.execute('INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (?,?,?,?,NULL,?)', (tid, now_at(), 'probe', os.getpid(), '/'))
    for s in stmts:
        c.execute(s)
    c.execute('COMMIT'); c.close(); return tid


class Cockpit:
    def __init__(self, which, trace=False, strace=False):
        self.which, self.bin, self.trace, self.strace = which, BIN[which], trace, strace
        self.home = tempfile.mkdtemp(prefix='sync-probe-cost2-home.', dir='/tmp'); HOMES.append(self.home)
        self.sock = 'sync-probe-cost2-%d-%d' % (os.getpid(), next(N)); SOCKS.append(self.sock)
        self.env = dict(CLEAN, CPD_HOME=self.home)

    def cpd(self, *a, timeout=30):
        return sp.run([self.bin, *a], env=self.env, capture_output=True, text=True, timeout=timeout)

    def start(self):
        open(self.home + '/config.toml', 'w').write('version = 1\nmachine = "probe"\n[tmux]\nsocket = "%s"\n' % self.sock)
        r = self.cpd('context', 'list'); assert r.returncode == 0, r.stderr
        r = tmux(self.sock, '-f', '/dev/null', 'new-session', '-d', '-s', 'x', '-x', '285', '-y', '50', 'exec timeout 900 sleep 800')
        assert r.returncode == 0, r.stderr
        n = 0
        def seed_step():
            write(self.home, SEED)
        seed_step()
        pre = 'exec timeout 900 '
        envs = 'env CPD_HOME=%s PATH=%s%s ' % (self.home, CLEAN['PATH'], ' CPD_TUI_TRACE=1' if self.trace else '')
        def wrap(v):
            if self.strace == 'engine':
                return 'strace -f -ttt -e trace=execve -o %s/strace-engine.txt ' % self.home if v == 'engine' else ''
            if self.strace:
                return 'strace -f -ttt -e trace=pread64 -o %s/strace-%s.txt ' % (self.home, v)
            return ''
        cmds = [('new-window', '-d', '-t', 'x', '-n', 'v', pre + wrap('tree') + envs + self.bin + ' tui view tree'),
                ('split-window', '-d', '-h', '-t', 'x:v', '-l', '250', pre + wrap('plan') + envs + self.bin + ' tui view plan'),
                ('split-window', '-d', '-h', '-t', 'x:v.1', '-l', '33', pre + wrap('widgets') + envs + self.bin + ' tui view widgets'),
                ('new-window', '-d', '-t', 'x', '-n', 'e', pre + wrap('engine') + envs + self.bin + ' watch')]
        for c in cmds:
            r = tmux(self.sock, *c); assert r.returncode == 0, (c, r.stderr)
        self.pids = {}
        for _ in range(200):
            self.pids = self.find()
            if len(self.pids) == 4:
                break
            time.sleep(0.1)
        assert len(self.pids) == 4, ('processes', self.pids)
        return self

    def find(self):
        found = {}
        for d in os.listdir('/proc'):
            if not d.isdigit():
                continue
            try:
                argv = open('/proc/%s/cmdline' % d, 'rb').read().split(b'\0')[:-1]
                argv = [a.decode() for a in argv]
                if os.readlink('/proc/%s/exe' % d) != self.bin or self.home.encode() not in open('/proc/%s/environ' % d, 'rb').read():
                    continue
            except OSError:
                continue
            if argv[1:3] == ['tui', 'view'] and len(argv) == 4:
                found[argv[3]] = int(d)
            elif argv[1:] == ['watch']:
                found['engine'] = int(d)
        return found

    def stop(self):
        tmux(self.sock, 'kill-server')
        r = tmux(self.sock, 'ls'); assert r.returncode != 0
        sp.run(['pkill', '-f', self.home], capture_output=True)
        time.sleep(0.3)
        shutil.rmtree(self.home, ignore_errors=True)


def status_cs(path):
    d = {}
    for l in open(path):
        if 'ctxt_switches' in l:
            k, v = l.split(':'); d[k] = int(v)
    return d['voluntary_ctxt_switches'] + d['nonvoluntary_ctxt_switches']


def statf(path):
    f = open(path).read(); return [int(x) for x in f[f.rindex(')') + 2:].split()[1:]]


def snap(pid):
    s = statf('/proc/%d/stat' % pid)
    d = {'cpu': s[10] + s[11], 'child': s[12] + s[13], 't': time.monotonic(), 'tid': {}}
    for l in open('/proc/%d/status' % pid):
        k = l.split(':')[0]
        if k in ('VmRSS', 'VmHWM', 'Threads'):
            d[k] = int(l.split(':')[1].split()[0])
    for t in os.listdir('/proc/%d/task' % pid):
        try:
            d['tid'][t] = (open('/proc/%d/task/%s/comm' % (pid, t)).read().strip(), status_cs('/proc/%d/task/%s/status' % (pid, t)), sum(statf('/proc/%d/task/%s/stat' % (pid, t))[10:12]))
        except OSError:
            pass
    d['main'] = status_cs('/proc/%d/status' % pid)
    return d


def cpu_total():
    v = [int(x) for x in open('/proc/stat').readline().split()[1:]]
    return sum(v), v[3] + v[4]   # total, idle+iowait


class Ticks(threading.Thread):
    """Tick starts: every change of the mtime of locks/watch.lock (the engine touches it at each tick start), 20 ms poll."""
    def __init__(self, home):
        super().__init__(daemon=True); self.p = home + '/locks/watch.lock'; self.stopped = False; self.t = []; self.last = None

    def run(self):
        while not self.stopped:
            try:
                m = os.stat(self.p).st_mtime_ns
                if m != self.last:
                    if self.last is not None:
                        self.t.append(m / 1e9)
                    self.last = m
            except OSError:
                pass
            time.sleep(0.02)


def trace_lines(home, v):
    try:
        return open('%s/run/tui/%s.log' % (home, v)).read().splitlines()
    except OSError:
        return []


def parse_trace(lines):
    out = []
    for l in lines:
        m = re.search(r'gap_us=(\d+) read_us=(\d+) layout_us=(\d+) draw_us=(\d+)', l)
        if m:
            out.append(tuple(int(x) for x in m.groups()))
    return out


def commit_loop(ck, seconds, rate, rec):
    """Real `cpd context note` commands at `rate` per second, on an absolute schedule, for `seconds`."""
    t0 = time.monotonic(); i = 0
    while True:
        due = t0 + i / rate
        if due - t0 >= seconds:
            break
        w = due - time.monotonic()
        if w > 0:
            time.sleep(w)
        a = time.monotonic()
        r = ck.cpd('context', 'note', '--context', 'ctx', 'load %d' % i, timeout=30)
        rec.append((time.time(), time.monotonic() - a, r.returncode, r.stderr[:200] if r.returncode else ''))
        i += 1


def pctl(v, p):
    v = sorted(v)
    if not v:
        return None
    k = (len(v) - 1) * p; f = int(k); c = min(f + 1, len(v) - 1); return v[f] + (v[c] - v[f]) * (k - f)


def window(ck, W, load=False):
    names = ck.pids
    tk = Ticks(ck.home); tk.start()
    tr0 = {v: trace_lines(ck.home, v) for v in ('tree', 'plan')} if ck.trace else {}
    time.sleep(0.1)
    la0 = open('/proc/loadavg').read().split()[:3]; c0 = cpu_total()
    a = {k: snap(p) for k, p in names.items()}
    rec = []
    th = None
    if load:
        th = threading.Thread(target=commit_loop, args=(ck, W, RATE, rec)); th.start()
    time.sleep(W)
    b = {k: snap(p) for k, p in names.items()}
    c1 = cpu_total(); la1 = open('/proc/loadavg').read().split()[:3]
    ticks = [t for t in tk.t]; tk.stopped = True
    if th:
        th.join()
    res = {'W': W, 'loadavg_start': la0, 'loadavg_end': la1,
           'machine_busy_pct': round(100 * (1 - (c1[1] - c0[1]) / (c1[0] - c0[0])), 1), 'ticks': len(ticks), 'procs': {}}
    for k in names:
        x, y = a[k], b[k]; dt = y['t'] - x['t']
        thr = {}
        for t, (c, n, u) in y['tid'].items():
            if t in x['tid']:
                thr[c] = thr.get(c, [0, 0]); thr[c][0] += (n - x['tid'][t][1]) / dt; thr[c][1] += 100 * (u - x['tid'][t][2]) / TCK / dt
        res['procs'][k] = {'cpu_pct': 100 * (y['cpu'] - x['cpu']) / TCK / dt, 'child_cpu_pct': 100 * (y['child'] - x['child']) / TCK / dt,
                           'wake_total': sum(v[0] for v in thr.values()), 'wake_main': (y['main'] - x['main']) / dt,
                           'rss_start_kib': x['VmRSS'], 'rss_end_kib': y['VmRSS'], 'hwm_kib': y['VmHWM'], 'threads': y['Threads'],
                           'threads_start': x['Threads'], 'per_thread': {c: [round(v[0], 2), round(v[1], 3)] for c, v in thr.items()}}
    if load:
        ok = [r for r in rec if r[2] == 0]
        res['commits'] = {'n': len(rec), 'failed': len(rec) - len(ok), 'first_error': next((r[3] for r in rec if r[2]), ''),
                          'wall_ms_p50': round(1000 * pctl([r[1] for r in rec], .5), 1) if rec else None,
                          'wall_ms_max': round(1000 * max(r[1] for r in rec), 1) if rec else None}
    if ck.trace:
        res['trace'] = {}
        for v in ('tree', 'plan'):
            rows = parse_trace(trace_lines(ck.home, v)[len(tr0[v]):])
            res['trace'][v] = {'frames': len(rows), 'draws_per_s': round(len(rows) / W, 2),
                               'frames_with_read_per_s': round(sum(1 for r in rows if r[1] > 0) / W, 2),
                               'frames_with_layout_per_s': round(sum(1 for r in rows if r[2] > 0) / W, 2),
                               'gap_ms_p50': round(pctl([r[0] for r in rows[1:]], .5) / 1000, 1) if len(rows) > 1 else None,
                               'gap_ms_max': round(max(r[0] for r in rows) / 1000, 1) if rows else None,
                               'read_us_max': max((r[1] for r in rows), default=None)}
    ts = ticks
    res['tick_intervals_s'] = [round(ts[i + 1] - ts[i], 3) for i in range(len(ts) - 1)][:400]
    return res


def burst(ck):
    """100 commits within 1 s (SQL writes of one `context-record` Event each, like `context note`), 3 s before, 6 s after."""
    tk = Ticks(ck.home); tk.start(); time.sleep(3)
    t0 = time.time(); m0 = time.monotonic(); lat = []
    for i in range(100):
        due = m0 + i * 0.0099
        w = due - time.monotonic()
        if w > 0:
            time.sleep(w)
        a = time.monotonic()
        write(ck.home, ["INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES (cpd_txn(),'%s','context-record','ctx',NULL,NULL,'b%d','{}')" % (now_at(), i)])
        lat.append(time.monotonic() - a)
    t1 = time.time()
    time.sleep(6); tk.stopped = True
    return {'t0': t0, 't1': t1, 'ticks': tk.t, 'write_ms_p50': round(1000 * pctl(lat, .5), 2), 'write_ms_max': round(1000 * max(lat), 2)}


def strace_snaps(home, v):
    MIN = 3 if v == 'widgets' else 15
    """Snapshot bursts: consecutive pread64 of one thread with gaps <= 3 ms; a snapshot has >= 15 of them. Returns start times."""
    per = {}
    try:
        for l in open('%s/strace-%s.txt' % (home, v)):
            p = l.split(None, 2)
            if len(p) > 2 and p[2].startswith('pread64'):
                per.setdefault(p[0], []).append(float(p[1]))
    except OSError:
        return []
    out = []
    for tid, ts in per.items():
        start = ts[0]; n = 1
        for a, b in zip(ts, ts[1:]):
            if b - a > 0.003:
                if n >= MIN:
                    out.append(start)
                start = b; n = 0
            n += 1
        if n >= MIN:
            out.append(start)
    return sorted(out)


def run_kind(kind, which, rnd):
    t = time.time(); log('start', kind, which, rnd)
    ck = Cockpit(which, trace=(kind in ('load', 'burst', 'stop', 'loadread')), strace=(kind in ('burst', 'loadread')))
    rec = {'kind': kind, 'bin': which, 'round': rnd, 'pids': None, 'home': ck.home}
    try:
        ck.start(); rec['pids'] = ck.pids; time.sleep(SETTLE)
        if kind == 'idle':
            rec.update(window(ck, IDLE_W))
        elif kind == 'load':
            rec.update(window(ck, LOAD_W, load=True))
        elif kind == 'burst':
            time.sleep(5)
            tr0 = {v: len(parse_trace(trace_lines(ck.home, v))) for v in ('tree', 'plan')}
            b = burst(ck); rec.update({k: v for k, v in b.items() if k != 'ticks'}); rec['ticks_abs'] = b['ticks']
            rec['tick_in_burst_s'] = sum(1 for x in b['ticks'] if b['t0'] <= x <= b['t0'] + 1.0)
            rec['tick_max_per_1s'] = max([sum(1 for y in b['ticks'] if x <= y < x + 1.0) for x in b['ticks']] or [0])
            rec['tick_total_10s'] = len(b['ticks'])
            tr = {}
            for v in ('tree', 'plan'):
                tr[v] = len(parse_trace(trace_lines(ck.home, v))) - tr0[v]
            rec['trace_frames'] = tr
            rec['snaps'] = {v: [round(x - b['t0'], 3) for x in strace_snaps(ck.home, v) if b['t0'] - 1 <= x <= b['t1'] + 3] for v in ('tree', 'plan', 'widgets')}
        elif kind == 'loadread':
            tk = Ticks(ck.home); tk.start(); cr = []; t0 = time.time()
            tr0 = {v: len(parse_trace(trace_lines(ck.home, v))) for v in ('tree', 'plan')}
            commit_loop(ck, LOAD_W, RATE, cr); t1 = time.time(); tk.stopped = True; time.sleep(1)
            rec.update({'W': LOAD_W, 'commits': len(cr), 'failed': sum(1 for r in cr if r[2]), 'ticks': [round(x - t0, 3) for x in tk.t],
                        'draws': {v: len(parse_trace(trace_lines(ck.home, v))) - tr0[v] for v in ('tree', 'plan')},
                        'snaps': {v: [round(x - t0, 3) for x in strace_snaps(ck.home, v) if t0 <= x <= t1] for v in ('tree', 'plan', 'widgets')}})
        elif kind == 'stop':
            for v in ('tree', 'plan', 'widgets'):
                os.kill(ck.pids[v], signal.SIGSTOP)
            tk = Ticks(ck.home); tk.start(); cr = []
            commit_loop(ck, 20, RATE, cr); tk.stopped = True
            for v in ('tree', 'plan', 'widgets'):
                os.kill(ck.pids[v], signal.SIGCONT)
            time.sleep(1)
            rec.update({'stopped_s': 20, 'ticks': len(tk.t), 'ticks_abs': [round(x - tk.t[0], 3) for x in tk.t], 'commits': len(cr), 'failed': sum(1 for r in cr if r[2]),
                        'wall_ms_p50': round(1000 * pctl([r[1] for r in cr], .5), 1), 'wall_ms_max': round(1000 * max(r[1] for r in cr), 1),
                        'views_alive_after': {v: os.path.exists('/proc/%d' % ck.pids[v]) for v in ('tree', 'plan', 'widgets')}})
    except Exception as e:
        rec['error'] = repr(e)
        log('ERROR', kind, which, rnd, repr(e))
    finally:
        ck.stop()
    rec['elapsed_s'] = round(time.time() - t, 1)
    with open(OUT + '/results.jsonl', 'a') as f:
        f.write(json.dumps(rec) + '\n')
    log('done', kind, which, rnd, rec.get('error', 'ok'), rec['elapsed_s'])


if __name__ == '__main__':
    try:
        for r in range(1, ROUNDS + 1):
            for kind in KINDS:
                for which in ('before', 'after'):
                    run_kind(kind, which, r)
    finally:
        cleanup()
```

`analyze.py`:

```python
import json, sys, statistics as st, collections
R = [json.loads(l) for l in open(sys.argv[1] + '/results.jsonl')]
R = [r for r in R if 'error' not in r]
def merge(ts, gap=0.0):
    out = []
    for x in ts:
        if not out or x - out[-1] > gap: out.append(x)
    return out
def from_intervals(iv):
    t = [0.0]
    for i in iv: t.append(t[-1] + i)
    return merge(t)
def med(v): return st.median(v)
def rng(v): return '%s to %s' % (fmt(min(v)), fmt(max(v)))
def fmt(x): return ('%.3f' % x) if abs(x) < 10 else ('%.1f' % x)
PROCS = ('tree', 'plan', 'widgets', 'engine')
def sel(kind, b): return [r for r in R if r['kind'] == kind and r['bin'] == b]
for kind in ('idle', 'load'):
    print('\n### %s windows' % kind)
    print('| Process | Bin | n | CPU % median (range) | child CPU % median | wakeups/s total median (range) | main thread wakeups/s median | RSS MiB start / end (median) | threads |')
    print('|---|---|---|---|---|---|---|---|---|')
    for p in PROCS:
        for b in ('before', 'after'):
            w = sel(kind, b)
            if not w: continue
            g = lambda k: [x['procs'][p][k] for x in w]
            print('| %s | %s | %d | %s (%s) | %s | %s (%s) | %s | %.1f / %.1f | %s |' % (p, b, len(w), fmt(med(g('cpu_pct'))), rng(g('cpu_pct')), fmt(med(g('child_cpu_pct'))),
                fmt(med(g('wake_total'))), rng(g('wake_total')), fmt(med(g('wake_main'))), med(g('rss_start_kib')) / 1024, med(g('rss_end_kib')) / 1024, sorted(set(g('threads')))))
    print('\nper-thread (median of windows): wakeups/s, CPU %')
    for p in PROCS:
        for b in ('before', 'after'):
            w = sel(kind, b)
            if not w: continue
            names = sorted({c for x in w for c in x['procs'][p]['per_thread']})
            print('- %s %s: ' % (p, b) + '; '.join('%s %.2f/s %.3f%%' % (c, med([x['procs'][p]['per_thread'].get(c, [0, 0])[0] for x in w]), med([x['procs'][p]['per_thread'].get(c, [0, 0])[1] for x in w])) for c in names))
    print('\nrun context:')
    for x in sorted([r for r in R if r['kind'] == kind], key=lambda r: (r['round'], r['bin'])):
        tt = from_intervals(x['tick_intervals_s']); t = [round(tt[i+1]-tt[i],3) for i in range(len(tt)-1)]
        print('- r%d %s: loadavg %s to %s, machine busy %.1f %%, engine ticks %d (%.3f/s), min interval %s s%s' % (x['round'], x['bin'], ' '.join(x['loadavg_start']), ' '.join(x['loadavg_end']), x['machine_busy_pct'], len(tt), len(tt) / x['W'], min(t) if t else '-',
              (', commits %d (failed %d), wall p50 %s ms max %s ms' % (x['commits']['n'], x['commits']['failed'], x['commits']['wall_ms_p50'], x['commits']['wall_ms_max'])) if 'commits' in x else ''))
    if kind == 'load':
        print('\nframe trace (tree, plan) per window:')
        for x in sorted(sel('load', 'before') + sel('load', 'after'), key=lambda r: (r['round'], r['bin'])):
            print('- r%d %s: ' % (x['round'], x['bin']) + '; '.join('%s draws %.2f/s, frames with a read %.2f/s, gap p50 %s ms max %s ms, read_us max %s' % (v, d['draws_per_s'], d['frames_with_read_per_s'], d['gap_ms_p50'], d['gap_ms_max'], d['read_us_max']) for v, d in x['trace'].items()))
print('\n### loadread (strace, 60 s, 244 commits/min)')
for x in sorted([r for r in R if r['kind'] == 'loadread'], key=lambda r: (r['bin'], r['round'])):
    W = x['W']; t = merge(x['ticks'])
    mt = max([sum(1 for y in t if a <= y < a + 1.0) for a in t] or [0])
    s = []
    for v in ('tree', 'plan', 'widgets'):
        ts = x['snaps'][v]; bins = collections.Counter(int(a / 0.1) for a in ts); secs = collections.Counter(int(a) for a in ts)
        s.append('%s %d reads (%.2f/s, max %d per 100 ms, max %d per s)' % (v, len(ts), len(ts) / W, max(bins.values(), default=0), max(secs.values(), default=0)))
    print('- r%d %s: commits %d; engine ticks %d (%.2f/s, max %d in a second); draws tree %d plan %d; ' % (x['round'], x['bin'], x['commits'], len(t), len(t) / W, mt, x['draws']['tree'], x['draws']['plan']) + '; '.join(s))
print('\n### burst (100 commits in 1 s)')
for x in sorted([r for r in R if r['kind'] == 'burst'], key=lambda r: (r['bin'], r['round'])):
    s = []
    for v in ('tree', 'plan', 'widgets'):
        ts = x['snaps'][v]; bins = collections.Counter(int(a / 0.1) for a in ts if a >= 0); in1 = sum(1 for a in ts if 0 <= a <= 1.0)
        mx = max([sum(1 for b in ts if a <= b < a + 1.0) for a in ts] or [0])
        s.append('%s %d in burst second, max %d per 100 ms, max %d per 1 s, %d total in -1..+3 s' % (v, in1, max(bins.values(), default=0), mx, len(ts)))
    B = merge(x['ticks_abs'])
    print('- r%d %s: burst %.2f s, writes p50 %s ms max %s ms; engine ticks in burst second %d, max per second %d, %d in 10 s; draws tree %d plan %d; ' % (x['round'], x['bin'], x['t1'] - x['t0'], x['write_ms_p50'], x['write_ms_max'], sum(1 for y in B if x['t0'] <= y <= x['t0'] + 1.0), max([sum(1 for z in B if y <= z < y + 1.0) for y in B] or [0]), len(B), x['trace_frames']['tree'], x['trace_frames']['plan']) + '; '.join(s))
print('\n### stop (views SIGSTOPped 20 s, 244 commits/min)')
for x in sorted([r for r in R if r['kind'] == 'stop'], key=lambda r: (r['bin'], r['round'])):
    print('- r%d %s: engine ticks %d in 20 s, commits %d (failed %d), wall p50 %s ms max %s ms, views alive after SIGCONT %s' % (x['round'], x['bin'], x['ticks'], x['commits'], x['failed'], x['wall_ms_p50'], x['wall_ms_max'], x['views_alive_after']))
```

`summ.py`:

```python
import json, statistics as st
R=[json.loads(l) for l in open('out/results.jsonl')]
def g(kind,b,p,k): return [r['procs'][p][k] for r in R if r['kind']==kind and r['bin']==b]
def verdict(b,a,absmin=0.0):
    d=st.median(a)-st.median(b); sp=max(max(a)-min(a),max(b)-min(b),absmin)
    return d, ('ABOVE' if d>sp else ('below' if d<-sp else 'within spread')), sp
for kind in ('idle','load'):
    print('##',kind)
    for p in ('tree','plan','widgets','engine'):
        for k,lab,res in (('cpu_pct','CPU %',0.0084),('child_cpu_pct','child CPU %',0.0084),('wake_total','wake/s',0.0),('rss_end_kib','MiB',0.0),('threads','threads',0.0)):
            b,a=g(kind,'before',p,k),g(kind,'after',p,k)
            if k=='rss_end_kib': b=[x/1024 for x in b]; a=[x/1024 for x in a]
            d,v,sp=verdict(b,a,res)
            print('| %s | %s | %.3f (%.3f-%.3f) | %.3f (%.3f-%.3f) | %+.3f | spread %.3f | %s |'%(p,lab,st.median(b),min(b),max(b),st.median(a),min(a),max(a),d,sp,v))
```

`execs.py`:

```python
import sys; sys.argv=['x','/tmp/sync-probe-cost2/out-exec','1','idle']
import cost, time, collections, re, json
res = {}
for which in ('before', 'after'):
    ck = cost.Cockpit(which, trace=False, strace='engine').start()
    try:
        time.sleep(10); t0 = time.time(); rec = []
        cost.commit_loop(ck, 30, cost.RATE, rec); t1 = time.time(); time.sleep(0.5)
        c = collections.Counter()
        for l in open(ck.home + '/strace-engine.txt'):
            m = re.match(r'(\d+)\s+([\d.]+)\s+execve\("([^"]+)", \[([^\]]*)\]', l)
            if m and t0 <= float(m.group(2)) <= t1 and '= 0' in l:
                argv = m.group(4).replace('"', '').split(', ')
                c[' '.join([m.group(3).split('/')[-1]] + argv[1:3])] += 1
        res[which] = dict(c.most_common(12)); res[which + '_window_s'] = round(t1 - t0, 1)
    finally:
        ck.stop()
print(json.dumps(res, indent=1))
```

Cleanup: `pgrep -af sync-probe` shows none of this Job's processes; `tmux -L <each of the 45 sockets sync-probe-cost2-*> ls` failed ("no server running") and the socket files are removed; the throwaway homes, `/tmp/sync-probe-cost2` and the two git worktrees (`git worktree list` shows none) are removed.
