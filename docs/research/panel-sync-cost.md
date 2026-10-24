# Panel sync: baseline cost and engine lag

Job `cost`, Attempt 1, Plan `sync` (decision hs-d595 part 3). Findings only; no design. Taken 2026-10-23 on `hs`
(16 cores, `getconf CLK_TCK` = 100), repo at `c2953181`. Every number says its sample count. `verified` says how;
`unverified` and `guess:` mark the rest. My processes ran at nice 19, idle I/O. Times in the store are UTC; the
machine's clock is EDT (UTC-4).

## Summary

- Idle, median of 5 windows of 120 s: tree 0.37 % CPU, 10.2 wakeups/s, 19.7 MiB; plan 0.36 %, 10.0, 19.6 MiB;
  widgets 0.14 %, 5.0, 15.6 MiB; engine 0.50 %, 63, 17.8 MiB (plus 1.7 to 8.2 % of a core in waited-for children).
- The views' loop thread never waits on a read (`read_us` max 26 us, 8 106 frames, 6 runs); the reader thread costs
  about 10.5 ms CPU per commit (tree) and 4.6 ms (plan); 2 runs, see 2.
- The live store: 81 525 commits in 7 days (485/h mean); busiest minute 1 121 (a bulk archive), next 244;
  55.5 % of commits append no Event.
- The engine's tick starts every 6.0 s on average (n=50) because E5 (`pr poll`) blocks it; a `dialog` Inbox item
  opens a median 6.1 s (p95 12.6 s, n=229) after its cause Event.

## 1. Idle cost of the live views and the engine

Method: `/proc` only, two reads per window 120 s apart (script in 6); the highest Event id from
`sqlite3 -readonly ~/.cpd/cpd.db` before and after. Nothing attached to any process. I cannot know whether the User
pressed a key in any window. A window is idle for the store only when 0 Events were committed: windows 4 and 5.

**The live cockpit was restarted by someone else between windows 1 and 2** (views and engine new pids, age 126 s
at window 2; exact cause unverified). Window 1 measured the old processes (age 7 h 29 m, pids 4171796, 4171800,
4171807, 4172555), windows 2 to 5 the new ones (pids 2438032, 2438040, 2438045, 2438527). Which build the new
processes run is unverified: `~/.local/bin/cpd` reports `0.13.45-c2953181` now (file mtime 16:41:51, after the
restart); the old processes' build, `0.13.45-09b72595`, is inferred from the release binary's mtime (09:06).

| Window | Start (EDT) | Length | Events committed | Commits (txns) | loadavg 1 min, start to end |
|---|---|---|---|---|---|
| 1 | 16:32:57 | 120 s | 23 | 48 | 1.07 to 1.75 |
| 2 | 16:41:57 | 120 s | 29 | 123 | 6.99 to 5.28 |
| 3 | 16:50:57 | 120 s | 5 | 62 | 3.08 to 1.90 |
| 4 | 17:01:23 | 120 s | 0 | 21 | 2.38 to 1.47 |
| 5 | 17:08:23 | 120 s | 0 | 19 | 2.79 to 2.99 |

CPU % of one core, per window (1 to 5), then median and worst of the 5:

| Process | W1 | W2 | W3 | W4 | W5 | Median | Worst |
|---|---|---|---|---|---|---|---|
| tree | 0.900 | 0.700 | 0.367 | 0.342 | 0.325 | 0.367 | 0.900 |
| plan | 0.442 | 0.400 | 0.358 | 0.167 | 0.158 | 0.358 | 0.442 |
| widgets | 0.142 | 0.150 | 0.133 | 0.142 | 0.108 | 0.142 | 0.150 |
| engine (itself) | 0.500 | 0.517 | 0.542 | 0.467 | 0.442 | 0.500 | 0.542 |
| engine, waited-for children | n/a | n/a | 8.207 | 1.900 | 1.692 | 1.900 | 8.207 (n=3) |

The 10 ms tick of `utime` makes one window's value good to about 0.01 % (1 tick in 120 s = 0.0083 %).

Wakeups per second (voluntary + nonvoluntary context switches of the process), then RSS (KiB), threads, fds:

| Process | W1 | W2 | W3 | W4 | W5 | Median | Worst | VmRSS KiB (W5; HWM) | Threads | fds |
|---|---|---|---|---|---|---|---|---|---|---|
| tree | 10.32 | 11.82 | 10.11 | 10.08 | 10.22 | 10.22 | 11.82 | 20 204 (20 204) | 2 | 9 |
| plan | 10.02 | 10.82 | 10.22 | 10.01 | 10.01 | 10.02 | 10.82 | 20 056 (20 056) | 2 | 9 |
| widgets | 4.99 | 5.13 | 5.00 | 4.99 | 4.99 | 4.99 | 5.13 | 15 956 (16 096) | 2 | 10 |
| engine | 65.08 | 62.52 | 61.87 | 65.15 | 62.95 | 62.95 | 65.15 | 18 240 (18 240) | 1 to 3 | 10 to 12 |

Old processes (W1): RSS 16 536 / 16 692 / 15 144 / 19 808 KiB, 1 / 1 / 2 / 3 threads. RSS is stable over W2 to W5
(+40 KiB in 20 min). Per thread (W5, n=1 window): tree `cpd` loop 10.2 wakeups/s
and 0.09 %, `cpd-reads` 4.1 wakeups/s and 0.22 %; plan loop 10.0/s and 0.03 %, `cpd-reads` 4.3/s and 0.12 %;
widgets main 5.0/s and 0.11 %, second thread 1.0/s and 0 %; engine main thread holds all of it (63/s, 0.43 %).
The 10/s of the views are the 100 ms frame wait (`pace::wait_now`); the 4/s of `cpd-reads` is guess: the
200 ms poll plus its answers.

Since start (`/proc/<pid>/stat`, 17:12, ages 1 858 s for the new processes): tree 0.411 %, plan 0.265 %, widgets
0.140 %, engine 0.498 % (own) and 3.6 % (children waited for: cutime+cstime 5 119 ticks of 1 422 s). The widgets
view's child `nvidia-smi -lms 2000` (pid 2438049) used 0.144 % since start (stime 205 + utime 2 ticks, 1 426 s;
one reading, not a window). Old processes since start (age 26 935 s): tree 0.454 %, plan 0.389 %, widgets
0.134 %, engine 0.448 %.

Against the handoff (section 3, "0.1 to 0.4 % per view"): confirmed for the plan and widgets views, the tree is
0.33 to 0.90 % (median 0.37), and the engine is 0.5 % itself, plus up to 8 % in children.

## 2. The cost of one read on the live data size

Method: `.backup` copy of the live store (taken 16:32 EDT) in a throwaway `CPD_HOME` (`config.toml` of
`version = 1` and `machine = "probe"`), the installed `~/.local/bin/cpd tui view <v>` with `CPD_TUI_TRACE=1` inside
a private tmux server (`tmux -L sync-probe-cost-<pid> -f /dev/null`), each under `timeout 170`, 120 s windows,
one at a time. Commits: `cpd context note --context building-cpd` against the COPY, about one per second (115 for
the tree run, 116 for the plan run; the Event kind `context-record` is on the tree's and the plan's lists).
`cpd watch` never ran on the copy. The copy is a store frozen mid-work: its Sessions still show `working`.

Data size (copy, rows): events 44 496, jobs 1 759, sessions 1 539, inbox_items 1 806, pull_requests 373, txns 83 780,
history 60 610. Database 122 052 608 B (29 798 pages of 4 096 B); the copy has no WAL (`.backup` checkpoints). The
live WAL was 8 343 032 B at 16:32 and 16:51 (live file 122 036 224 B).

Frame trace, per frame of the view's loop (gap in us from start to start of draws):

| Run | Frames | gap_us median / p95 / max | draw_us median / p95 / max | read_us (frames >0) n / median / p95 / max | layout_us (>0) n / median / p95 / max |
|---|---|---|---|---|---|
| tree, no commits | 1 251 | 100 180 / 100 931 / 130 129 | 1 629 / 2 239 / 10 052 | 535 / 1 / 2 / 3 | 0 |
| tree, 115 commits | 1 251 | 100 003 / 102 466 / 129 998 | 1 461 / 4 464 / 22 546 | 617 / 1 / 2 / 21 | 0 |
| plan, no commits | 1 251 | 99 982 / 100 990 / 112 553 | 364 / 494 / 4 032 | 628 / 1 / 2 / 17 | 1 / 728 / 728 / 728 |
| plan, 116 commits | 1 251 | 99 989 / 100 948 / 103 354 | 377 / 494 / 2 721 | 614 / 1 / 2 / 19 | 117 / 476 / 608 / 794 |

Two earlier runs (150 s, 45 bursts of 3 commits, load 7.3, 1 551 frames each) agree: read_us max 21 (tree) and 26
(plan), gap max 136 112 us. No frame in any of the 6 runs had a gap above 150 ms.

**What the trace cannot show.** `read_us` is the time the loop thread spends inside reads, and the loop only takes
finished results from the reader thread (`reads.take()`, `tui/src/tree/mod.rs`; threads `cpd-reads` in `/proc`).
So `read_us` of at most 26 us says the loop never waited on a read (verified: 8 106 frames in 6 runs); it does
**not** say how long a snapshot or the Event poll takes, and the poll's query cannot be separated from the
snapshot's. Wall time of a snapshot is not measured (nothing in the trace holds it).

What I can give is CPU on the reader thread, from `/proc/<pid>/task/<tid>/stat` (10 ms ticks), the 120 s run with
commits minus the run without:

| View | Reader CPU, no commits | Reader CPU, with commits | Difference | Commits | CPU per commit |
|---|---|---|---|---|---|
| tree | 25 ticks (0.208 %) | 146 ticks (1.217 %) | 121 ticks | 115 | about 10.5 ms |
| plan | 15 ticks (0.125 %) | 68 ticks (0.567 %) | 53 ticks | 116 | about 4.6 ms |

The plan run has 117 snapshots, counted from 117 frames with `layout_us` > 0 (layout runs at a snapshot), so
the plan figure is per snapshot (verified). For the tree one snapshot per commit is an assumption (commits about 1.03 s
apart, coalesce 500 ms; unverified). Per-commit figures have +/- 1 tick of 115 = +/- 0.09 ms of rounding but only
2 runs (n=2); they include the poll's own query. Because the copy has `working` Sessions the tree draws every frame
(10 draws/s, loop thread 1.58 to 1.65 % in the copy against 0.09 % in the live tree): the live tree draws far less
(guess: no spinner on screen at the time). The copy's numbers for draw are therefore for a spinning tree.

Widgets on the copy (no trace; a commit of kind `context-record` is not in its list): 0.127 % CPU, 5.07 wakeups/s,
16 080 KiB, 2 threads (1 run of 150 s).

## 3. The live store's write rate

Method: `sqlite3 -readonly copy.db` of a `.backup` taken at 16:32 EDT (last commit 2026-10-23T20:32:32Z). A commit is
one row of `txns`: every write transaction inserts one (contract 4.3; verified in the `txns_own` trigger and by
`events.txn` joining to it). Window: 2026-10-16T20:32:32Z to 2026-10-23T20:32:32Z (168 h), n = 81 525 commits.

| UTC day | 16th (from 20:32) | 17th | 18th | 19th | 20th | 21st | 22nd | 23rd (to 20:32) |
|---|---|---|---|---|---|---|---|---|
| Commits | 598 | 2 120 | 8 600 | 8 392 | 24 377 | 15 710 | 16 772 | 4 956 |

Per UTC hour (169 hourly buckets including the 19 hours with no commit): mean 485.3/h, median 321, p95 1 861, min 0,
max 5 553 (2026-10-20T13Z, of which 5 090 were `watch` commits: cause unverified). The 150 hours with a commit have
median 396 and p95 1 913. Next busiest hours: 4 874 (2026-10-20T14Z), 2 988 (2026-10-21T16Z).

| Bucket | Buckets with a commit | Median | p95 | Max (when) |
|---|---|---|---|---|
| minute | 6 184 | 8 | 49 | 1 121 (2026-10-21T16:05Z); next 244 (2026-10-22T01:55Z) |
| second | 41 462 | 1 | 6 | 976 (2026-10-21T16:05:10Z) |

Percentiles: linear interpolation over the buckets that have a commit. The 976 commits of that second are 972
`session archive` and 4 others (verified by `group by command`): a bulk archive. Whole-window mean: 0.135 commits/s.

By command (n=81 525): `watch` 41 794, `session state` 12 505, `attempt run` 7 536, `pr poll` 3 791, `inbox reconcile` 2 466.
Events: 42 192 in 20 273 seconds that had one (median 1, p95 5, max 973 per second).

Commits that append no Event (txns with no `events` row, verified by the join): 45 254 = 55.5 %: `watch` 37 744,
`pr poll` 3 403, `attempt run` 3 392, `board take` 518, other 197. So `pr poll`'s `pull_requests` writes are 4.2 % of all
commits (3 403 of 3 791 `pr poll` commits write no Event, 90 %). The `watch` ones write `history` rows of `attempts`
(1 492 of the 1 494 history rows of the no-Event `watch` commits: updates of `attempts`); the rest is unverified.

Against the handoff ("76 in the busiest minute, 212 in the last hour"): the busiest minute of the last 7 days is 1 121
(one bulk archive), 244 the next; the busiest second apart from the archive is not computed (unverified).

## 4. The engine's lag from cause to Inbox item

Method: copy as in 3. For each item that has a stored cause (914 of the 1 806 items, all opened in the window) joined to `inbox_items.event` (the cause Event, set by `core/src/inbox/rules.rs`): lag = `at` of the
`inbox-opened` Event minus `at` of the cause Event. The Event's `at` is later than the item's `opened_at` in all 1 806 cases
(verified; 9 to 22 ms in the 4 rows I looked at), so Event `at` is the commit. Kinds whose items have no stored cause Event (`event`
is NULL in every row) are **dropped**: `plan-review` (122), `pr-failing` (275), `pr-ready` (438), `question` (3),
`review-requested` (45), `curate-docs` (5), `curate-retire` (4). `inbox-closed` Events store no cause Event for any
kind (`data` is `{item, kind}`), so a closing lag is only derived for `dialog` (below).

| Kind (cause Event kind) | n | min | median | p95 | max | <= 1 s | 1 to 5 s | > 5 s |
|---|---|---|---|---|---|---|---|---|
| `dialog` (session-state) | 229 | 0.017 s | 6.085 s | 12.557 s | 1 043 s | 8.7 % | 32.3 % | 59.0 % |
| `unread` (session-state) | 598 | 0.188 s | 10.002 s | 6 531 s | 58 834 s | 5.7 % | 20.1 % | 74.2 % |
| `engine-error` (error) | 85 | 0.005 s | 0.021 s | 530 s | 566 s | 61.2 % | 0 % | 38.8 % |
| `job-stuck` (job-failed) | 2 | 606.7 s | 607.1 s | 607.4 s | 607.4 s | 0 % | 0 % | 100 % |

Reading it:
- `dialog` is the clean case (condition is `state = blocked` at the cause Event). Lags spread flat over 0 to 12 s
  (1 s bins, n=229: 20, 17, 10, 23, 24, 18, 20, 25, 21, 17, 12, 5 and 17 above 12 s). Mean 11.6 s (outliers),
  p75 8.7 s, p90 10.8 s; 95.2 % within 12.6 s. Closing (derived from the next `session-state` of that session with
  `from = blocked`; guess: that Event is what closes it): n=229, min 0.084 s, median 5.910 s, p95 13.982 s, max 2 318 s.
- `unread` is not an engine lag: its condition also needs the Session outside the chat slot, so the lag includes the
  User's own moves (max 16 h). Its first-per-cause subset (n=472) has median 7.6 s. Use it as an upper bound only.
- `engine-error`: the engine writes the `error` Event itself, so n=56 first openings have median 0.010 s (max 6.8 s);
  the 29 later ones are the same cause re-opened minutes later (not lag). `job-stuck` (n=2): 607 s is a time
  threshold (guess: about 10 min) and no tick lag.
- Fit to the 5 s tick: **no, not a uniform [0, 5 s].** The tick starts every 6.0 s on average (5 for ticks without a
  poll, 7.0 s for ticks with it, see 5), so a cause waits a residual time of E[T^2]/(2 E[T]) = 3.1 s on average
  (computed from the 50 intervals) before the tick starts, plus the time from the tick's start to E8, which runs after
  E5's blocking `pr poll`. The median of 6.1 s and the flat spread to 12 s fit "residual wait of 0 to about 8 s plus an
  E8 offset of about 3 s"; the E8 offset itself is not measured (unverified).

Handoff ("the Inbox up to about 5 s later"): corrected to a median of 6.1 s and a p95 of 12.6 s for `dialog`.
The cause-to-view part (about 0.7 s) is not measured here.

## 5. The engine's tick

- Cadence: `engine.rs` runs the steps, then waits `min(began + 5 s, E5's due time)` in naps of 100 ms (verified,
  source). The watched mtime of `~/.cpd/locks/watch.lock` (touched at each tick's start; stat every 100 ms for
  300 s, 16:42 EDT, n=51 starts, 50 intervals) gave: mean 6.02 s, median 5.75 s, min 4.96 s, max 8.67 s; 24 intervals
  of 4.99 s (mean) and 26 of 5.7 to 8.67 s (mean 6.97 s), alternating. Resolution +/- 0.1 s. One window.
- E5 runs `cpd pr poll` as a child process and waits for it (`engine/src/watch/pr.rs`, `Command::new`): the long
  ticks fit that (guess: a poll takes 1 to 4 s of wall time). It blocks E8 and the other steps behind it.
- The engine's own CPU is 0.44 to 0.54 % (n=5), all on its main thread. The 10 wakeups/s of the 100 ms nap
  do not explain the 63/s measured (cause unverified; a 3 s probe at tick time saw 99/s).
- The waited-for children cost more than the engine: 1.7 to 8.2 % of a core in 3 windows, 3.6 % since start (engine
  own 0.50 %). `/proc` does not say which step spawned them (E5's `cpd pr poll`, `gh`, `tmux` calls and others), so the
  tick's share against E5's share cannot be split from `/proc` alone (unverified; nothing was instrumented).
- `watch.log` (12 746 lines since 2026-10-15, read-only) logs only on action (lines per step: E8 2 452, E3 2 417, E5
  2 184 of which 2 110 "polled", E2 1 550, E10 1 325, E4 457, E12 421, E7 255, E9 191, E13 56, E1 10; plus 812
  "gh search failed transiently"). A quiet tick logs nothing, so the log gives no tick duration. 3 791 `pr poll` commits in 7 days (one per 2.7 min).

## 6. Baseline a later implementation must not exceed, and how to repeat it

Idle, live cockpit, median (worst) of 5 windows of 120 s (window 1 is the old processes, see 1):

| Process | CPU % of a core | Wakeups/s | VmRSS MiB | Threads |
|---|---|---|---|---|
| tree view | 0.37 (worst 0.90) | 10.2 (worst 11.8) | 19.7 | 2 |
| plan view | 0.36 (worst 0.44) | 10.0 (worst 10.8) | 19.6 | 2 |
| widgets view | 0.14 (worst 0.15) | 5.0 (worst 5.1) | 15.6 | 2 |
| engine | 0.50 (worst 0.54), children 1.9 (worst 8.2) | 63 (worst 65) | 17.8 | 1 to 3 |

Also the reader-thread CPU per commit: tree about 10.5 ms, plan about 4.6 ms; loop-thread `read_us` max 26 us;
frame gap median 100 ms, p95 about 101 ms; Inbox `dialog` lag median 6.1 s, p95 12.6 s.

Commands (all read-only; the first needs only `/proc`; `mini.py` is the script below, `sqlite3 -readonly` the
store):

```
# 1. idle cost: run 3 or more times at different times (default pids: pgrep of the live views and engine)
nice -n19 timeout 200 python3 mini.py 120
# 2. backup, private home, trace run (one view; repeat for tree and plan; tmux -L is a private server)
mkdir -p /tmp/p/home && sqlite3 ~/.cpd/cpd.db ".backup /tmp/p/home/cpd.db"
printf 'version = 1\nmachine = "probe"\n' > /tmp/p/home/config.toml
S=sync-probe-$$; tmux -L $S -f /dev/null new-session -d -s p -x 200 -y 50 \
  "env CPD_HOME=/tmp/p/home CPD_TUI_TRACE=1 timeout 170 cpd tui view tree"
sleep 5; PID=$(pgrep -f '^cpd tui view tree$' -n)      # the probe's; check its parent is the $S pane
( while :; do CPD_HOME=/tmp/p/home timeout 20 cpd context note --context building-cpd x; sleep 1; done ) &  # COPY only
nice -n19 timeout 160 python3 mini.py 120 tree=$PID     # reader-thread CPU is the "tid cpd-reads" line
kill %1; tmux -L $S kill-server; tmux -L $S ls          # must fail; trace: /tmp/p/home/run/tui/tree.log
# 3. write rate (per bucket: replace 16 with 13 for hours, 19 for seconds; no-Event share: join events on txn)
sqlite3 -readonly /tmp/p/home/cpd.db "select substr(at,1,16), count(*) from txns
  where at >= '2026-10-16T20:32:32' group by 1 order by 2 desc limit 5;
  select count(*) from txns t where not exists (select 1 from events e where e.txn=t.id);"
# 4. lag (per item; run on a backup copy, not the live file)
sqlite3 -readonly /tmp/p/home/cpd.db "select i.kind, (julianday(o.at)-julianday(k.at))*86400
  from inbox_items i join events o on o.kind='inbox-opened' and json_extract(o.data,'$.item')=i.id
  join events k on k.id=i.event"
# 5. tick starts: stat -c %.9Y ~/.cpd/locks/watch.lock every 100 ms for 300 s, print the distinct values
```

Percentiles are linear interpolation over the query output, done in Python. `mini.py` (run for 2 s and 3 s on the live pids):

```python
#!/usr/bin/env python3
# usage: mini.py SECS [name=pid,...]   (default: the live tree/plan/widgets views and the engine)
import os,re,subprocess as sp,sys,time
T=os.sysconf('SC_CLK_TCK'); W=float(sys.argv[1])
def pids():
    r={}
    for l in sp.run(['pgrep','-af',r'^cpd (tui view \w+|watch)$'],capture_output=True,text=True).stdout.splitlines():
        p,c=l.split(' ',1); r[c.split()[-1] if 'view' in c else 'engine']=int(p)
    return r
P=dict((k,int(v)) for k,v in (a.split('=') for a in sys.argv[2].split(','))) if len(sys.argv)>2 else pids()
def cs(path): return sum(int(l.split()[1]) for l in open(path) if 'ctxt_switches' in l)
def st(path): f=open(path).read(); return [int(x) for x in f[f.rindex(')')+2:].split()[1:]]
def snap(p):
    s=st(f'/proc/{p}/stat'); d={'cpu':s[10]+s[11],'child':s[12]+s[13],'start':s[18],'cs':cs(f'/proc/{p}/status'),'fds':len(os.listdir(f'/proc/{p}/fd')),'tid':{}}
    for l in open(f'/proc/{p}/status'):
        if l.split(':')[0] in('VmRSS','VmHWM','Threads'): d[l.split(':')[0]]=int(l.split(':')[1].split()[0])
    for t in os.listdir(f'/proc/{p}/task'):
        d['tid'][t]=(open(f'/proc/{p}/task/{t}/comm').read().strip(),cs(f'/proc/{p}/task/{t}/status'),sum(st(f'/proc/{p}/task/{t}/stat')[10:12]))
    return d
ev=lambda: sp.run(['sqlite3','-readonly',os.path.expanduser('~/.cpd/cpd.db'),'select max(id) from events'],capture_output=True,text=True).stdout.strip()
a={k:snap(p) for k,p in P.items()}; e0=ev(); time.sleep(W); b={k:snap(p) for k,p in P.items()}; e1=ev()
print('window',W,'s  events committed',int(e1)-int(e0),' loadavg',open('/proc/loadavg').read().split()[:3])
for k in P:
    x,y=a[k],b[k]; up=float(open('/proc/uptime').read().split()[0])-y['start']/T
    print(k,P[k],'cpu%%=%.3f child_cpu%%=%.3f wake/s=%.2f RSS_KiB=%d HWM_KiB=%d threads=%d fds=%d age_s=%d total_cpu%%=%.3f'%(100*(y['cpu']-x['cpu'])/T/W,100*(y['child']-x['child'])/T/W,(y['cs']-x['cs'])/W,y['VmRSS'],y['VmHWM'],y['Threads'],y['fds'],up,100*y['cpu']/T/up))
    for t,(c,n,u) in y['tid'].items():
        if t in x['tid']: print('   tid',t,c,'wake/s=%.2f cpu%%=%.3f'%((n-x['tid'][t][1])/W,100*(u-x['tid'][t][2])/T/W))
```

## 7. What could not be measured; limits

- Snapshot wall time and the Event poll's query time (the trace holds neither, see 2); the CPU of a reader thread is
  a difference of two runs.
- Widgets has no frame trace. Pane titles and tmux option writes (`@cpd_*`, `set_pane_title`) and their cost were
  not measured; neither was the time from commit to a drawn view or to a pane title (nothing in the logs holds it).
- The engine's cost per step (E5 against the rest): no instrumentation was allowed; `/proc` splits only the engine
  and its children as a whole.
- The Inbox lag of kinds with no stored cause Event (7 kinds, 892 items); the closing lag of all kinds but `dialog`.
- Windows are not truly idle: the User may have pressed keys (unknowable); windows 1 to 3 had 23, 29 and 5 Events and
  48, 123 and 62 commits; the 2 windows with 0 Events still had 21 and 19 commits (engine, `pr poll`). The engine's
  own cost includes its work for the 5 Workers running at the time.
- Window 1 measured processes that were restarted before window 2, probably on another build; its numbers stand for the
  old processes only. Only 5 windows over 36 minutes, one afternoon; no night or weekend windows.
- Nice 19 and idle I/O: my samplers and commit loop may have waited for nice-0 runnable processes (load 1.5 to 10 on
  16 cores, per window above). The figures are the observed processes' own counters, so my niceness does not enter
  them; my commit loop's timing may. Probes ran during windows 2 and 3 and added load (guess: no effect on the
  observed processes' own CPU).
- The probes ran the installed binary on a copy with stale `working` Sessions (the tree draws every frame there).
- The 7-day figures include the busiest days of this project (20 to 22 October): not a typical week.

Cleanup: no `cpd watch` ran on a copy; nothing under `/home/clayton/.cpd` was written (reads only: `/proc`,
`sqlite3 -readonly`, `.backup`, `stat`, `cat`); the private tmux servers were killed (`tmux -L <socket> ls` failed,
`pgrep -af sync-probe` showed none of mine); `/tmp/sync-probe-cost` (the copy, the throwaway home, scripts) was
deleted.
