# Moon loader cost: CPU before and after the 50 ms trial

## Summary

- Tree, release, CPU % of one core before -> after (median of 3): tree A 0.91 -> 1.69 (x1.85), tree B 0.89 -> 1.71 (x1.93); debug: tree A 2.14 -> 4.31 (x2.02).
- Plan view, release: plan A1 2.54 -> 4.88 (x1.92), plan A2 2.46 -> 5.08 (x2.07), plan B 2.57 -> 5.06 (x1.97); debug: plan A1 4.83 -> 9.77 (x2.02).
- pi pane (real pane, node process, nice 19): working 4.08 -> 5.26 (x1.29, +1.18 points, 3 runs, range 5.17-6.23 after); idle control 0.43 -> 0.34 (1 run each).
- The cockpit idle controls (plan D, tree D) did not change (0.00 -> 0.03 and 0.11 -> 0.11 release): only a view with a spinner on screen costs more.
- In plain words: one cockpit tree with a spinner does about 0.8 % more of a core; one plan view about 2.3-2.6 % more; one working pi pane about 1.2 % more (release).
- The 3 % line is that of [anim-perf-after.md](anim-perf-after.md) ("Left"), not this Plan's; the tree's debug build in that doc was already above it. Here: plan view release 4.88-5.08 is above it (before 2.46-2.57: below), tree release 1.69-1.71 is below it, tree debug 4.31 is above it (before 2.14: below), pi pane 5.26 is above it (before 4.08: above).
- At 50 ms the draw gap p50 is 49.8-50.2 ms in every cockpit scenario with a spinner, no gap above 150 ms and no skipped frame in any A-type run (before: 99.9-100.2 ms, none).
- The pi pane showed only the new glyphs (U+E3C8..U+E3E3) in the pill and the closing row; no old nf-md moon (see `## After`).
- Limits: one machine, one base, one model, nice 19 and not 0 (unverified at nice 0); the load (2-6, mostly about 3) is far below the User's usual, so CPU is optimistic; before and after were not at the same load; pi frame rate not read from the pane (two captures a run), pi idle is 1 run; the 3-run ranges are in the tables.

## Method

Job moon-before-ui, attempt 1. BASELINE = the unchanged code: the cockpit with the 8 nf-md moons at 100 ms (`git diff --stat $(git merge-base HEAD origin/main)..HEAD -- tui core cli engine` listed no file before the measurement, so no production file of the binary differs from `origin/main`). The method is that of [anim-perf-after.md](anim-perf-after.md) ("Method", "Before and after"), [anim-perf-plan.md](anim-perf-plan.md) ("Method", steps 1-6) and [anim-perf-tree.md](anim-perf-tree.md), which are an earlier build: none of their numbers is used here. The frame trace is docs/commands.md section 17 "Frame trace" (`CPD_TUI_TRACE=1`, one line per draw `trace gap_us=<n> read_us=<n> layout_us=<n> draw_us=<n> frame=<n>` in `$CPD_HOME/run/tui/<view>.log`).

Differences from those docs, all deliberate: 3 runs per scenario (D: 1), each on a fresh copy of ONE base that is kept; the trace is cut to the measured window by the lines' timestamps (the 3 s wait and the end are not counted), so "draws" are the draws of the window and the gap of the window's first draw is counted (it is a real gap); skipped frames use `mod 8` (the old 8-frame loader). The after Job runs the same text with `$BIN` set to its own binary; nothing else changes (the skipped-frames modulus in `parse.py` becomes the new frame count, 28, and the doc of that Job says so).

Scenarios (Plan `quality`, still the largest Plan of the store: 213 Jobs (147 merged, 66 cancelled), 50 PRs, 46 with a pull request):

- plan A1: the Lead of `quality` is `working`, no running Job (only the PR title dots spin).
- plan A2: A1 plus the newest merged Job of `quality` set to `running` (and `merged_sha` NULL).
- plan B: A2 with the store write-locked 5 times, 3 s each, 2 s apart, from t = 2 s into the window (`locker.py`).
- plan D: the Lead `stopped`, no running Job: no spinner (the control).
- tree A: the Lead of `quality` `working` (a spinner on the Lead's dot).
- tree B: tree A with the store write-locked as in plan B.
- tree D: every Session `stopped` (no spinner; the control).

The machine, the `nice` of the runs (19, inherited from the Worker session: unprivileged, so it cannot be lowered; the real views run at 0 per anim-perf-plan.md, unverified here) and the load are in the tables. Windows: 35 s (plan D 30 s, tree D 35 s) after a 3 s wait. The view's pid is found under the tmux pane (`cpd` by `/proc/<pid>/comm`); CPU % is (utime + stime ticks of `/proc/<pid>/stat`, 100 per s) over the window.

The base was made at 2026-10-24 16:30 local (20:30 UTC) from `~/.cpd`, which was live (the Events of the Worker sessions kept being written, so the copy is a point in time; the real store was never written). Its sessions and Events are those of that moment. The after Job must use the same directory.

### Cockpit

Order: base (once), builds, then `matrix.sh` per profile. Every script is below, verbatim, as run from `/tmp/cpd-moon-anim/`. `WT` is the worktree, `BIN` the binary.

Builds (the machine had sccache; the release build took 41 s, the debug build 22 s):

```sh
scripts/cargo.sh build --release -p cpd     # target/release/cpd
scripts/cargo.sh build -p cpd               # target/debug/cpd
```

Base preparation, once (`prep-base.py`: `sqlite3.Connection.backup` of `~/.cpd/cpd.db` in read-only mode, plus `config.toml`, `run/` and `locks/`; all triggers dropped in the copy only; the Lead of `quality` un-archived and a `session-moved` Event `{"place":"chat"}` for it, so the plan view shows its Plan; the real cockpit's `tui/plan.log` and `tree.log` not copied):

```python
#!/usr/bin/env python3
# Prepares the ONE base of the moon-loader-cost runs at /tmp/cpd-moon-anim/base/.
# Reads ~/.cpd (never writes it). Run once; refuses to overwrite an existing base.
import os, shutil, sqlite3, sys, datetime
SRC = os.path.expanduser("~/.cpd")
BASE = "/tmp/cpd-moon-anim/base"
PLAN = "quality"
if os.path.exists(BASE):
    sys.exit("base exists: " + BASE)
os.makedirs(BASE)
src = sqlite3.connect("file:%s/cpd.db?mode=ro" % SRC, uri=True)
dst = sqlite3.connect(BASE + "/cpd.db")
src.backup(dst)
src.close()
shutil.copy(SRC + "/config.toml", BASE + "/config.toml")
shutil.copytree(SRC + "/run", BASE + "/run", symlinks=True)
shutil.copytree(SRC + "/locks", BASE + "/locks", symlinks=True)
# the view logs of the real cockpit are not part of the base
for f in ("plan.log", "tree.log"):
    p = BASE + "/run/tui/" + f
    if os.path.exists(p):
        os.remove(p)
# the history triggers call cpd_txn(), which only the engine defines: drop ALL triggers (copy only)
dst.create_function("cpd_txn", 0, lambda: 1)
for (name,) in dst.execute("select name from sqlite_master where type='trigger'").fetchall():
    dst.execute('drop trigger "%s"' % name)
lead, = dst.execute("select lead_session from plans where id=?", (PLAN,)).fetchone()
# the Lead of PLAN is archived and stopped in the real store: un-archive it, and make it the chat slot's
dst.execute("update sessions set archived_at=NULL where id=?", (lead,))
now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")
txn = dst.execute("select max(id)+1 from txns").fetchone()[0]
dst.execute("insert into txns(id,at,command,pid,session,cwd) values(?,?,?,?,NULL,'/')", (txn, now, "base", 1))
ctx, = dst.execute("select context from plans where id=?", (PLAN,)).fetchone()
dst.execute("insert into events(txn,at,kind,context,plan,session,text,data) values(?,?,?,?,?,?,?,?)",
            (txn, now, "session-moved", ctx, PLAN, lead, "Lead is in the chat slot.", '{"place":"chat"}'))
dst.commit()
dst.execute("pragma wal_checkpoint(truncate)")
dst.close()
print("base made", now, "lead", lead, "plan", PLAN)
```

```sh
python3 /tmp/cpd-moon-anim/prep-base.py     # refuses to overwrite /tmp/cpd-moon-anim/base
```

Scenario setup, on a copy (`scenario.py`; the base has the Lead `stopped` and no running Job of the Plan):

```python
#!/usr/bin/env python3
# usage: scenario.py <copy> <plan-A1|plan-A2|plan-B|plan-D|tree-A|tree-B|tree-D>
# Sets the Session and Job state of ONE copy of the base. The base has the Lead stopped and no running Job of the plan.
import sqlite3, sys
copy, scn = sys.argv[1], sys.argv[2]
PLAN = "quality"
db = sqlite3.connect(copy + "/cpd.db")
lead, = db.execute("select lead_session from plans where id=?", (PLAN,)).fetchone()
def working():
    db.execute("update sessions set state='working', state_label=NULL where id=?", (lead,))
def running_job():
    seq, = db.execute("select seq from jobs where plan=? and status='merged' order by seq desc limit 1", (PLAN,)).fetchone()
    db.execute("update jobs set status='running', merged_sha=NULL where seq=?", (seq,))
if scn == "plan-A1": working()
elif scn in ("plan-A2", "plan-B"): working(); running_job()
elif scn == "plan-D": pass
elif scn in ("tree-A", "tree-B"): working()
elif scn == "tree-D": db.execute("update sessions set state='stopped', state_label=NULL where state<>'stopped'")
else: sys.exit("unknown scenario " + scn)
db.commit(); db.close()
```

Scenario B's store lock (`locker.py`):

```python
#!/usr/bin/env python3
# B: write-locks the store 5 times: BEGIN IMMEDIATE, hold 3 s, COMMIT, 2 s apart. usage: locker.py <copy>
import sqlite3, sys, time
db = sqlite3.connect(sys.argv[1] + "/cpd.db", isolation_level=None, timeout=30)
for _ in range(5):
    db.execute("BEGIN IMMEDIATE"); time.sleep(3); db.execute("COMMIT"); time.sleep(2)
db.close()
```

One run (`run.sh`): a fresh copy of the base, the scenario, a private tmux server, the window, the CPU read, the trace parsed. `unset TMUX` is implied by `env -i`; `TMUX_TMPDIR` is a new directory; a `trap` kills the server `moon-anim-before` and removes the copy on any exit.

```sh
#!/bin/bash
# usage: run.sh <binary> <profile> <view plan|tree> <scenario A1|A2|B|D|A> <run n> <window s>
# One run on a fresh copy of the base; prints one JSON line (also appended to results/<profile>.jsonl).
set -u
BIN=$1 PROF=$2 VIEW=$3 SCN=$4 N=$5 WIN=$6
D=/tmp/cpd-moon-anim
COPY=$(mktemp -d /tmp/cpd-moon-anim/copy.XXXXXX)
TT=$(mktemp -d /tmp/cpd-moon-anim/tmux.XXXXXX)
S="env -i PATH=$PATH HOME=$HOME TMUX_TMPDIR=$TT tmux -L moon-anim-before -f /dev/null"
LOCK=0
cleanup() { [ "$LOCK" != 0 ] && kill "$LOCK" 2>/dev/null; $S kill-server 2>/dev/null; rm -rf "$COPY" "$TT"; }
trap cleanup EXIT
cp -a --reflink=auto $D/base/. "$COPY"/
rm -f "$COPY/run/tui/$VIEW.log"
python3 $D/scenario.py "$COPY" "$VIEW-$SCN" || exit 1
$S new-session -d -x 200 -y 50 -e CPD_HOME="$COPY" -e CPD_TUI_TRACE=1 "$BIN tui view $VIEW"
sleep 3
PID=$($S list-panes -F '#{pane_pid}')
while [ "$(cat /proc/$PID/comm 2>/dev/null)" != cpd ]; do
  C=$(pgrep -P "$PID" | head -1); [ -z "$C" ] && { echo "no cpd process" >&2; exit 1; }; PID=$C
done
ticks() { awk '{print $14+$15}' /proc/$PID/stat; }
T0=$(date +%s.%N); K0=$(ticks)
if [ "$SCN" = B ]; then ( sleep 2; exec python3 $D/locker.py "$COPY" ) & LOCK=$!; fi
sleep "$WIN"
K1=$(ticks); T1=$(date +%s.%N)
NI=$(ps -o ni= -p $PID | tr -d ' ')
LOAD=$(cut -d' ' -f1-3 /proc/loadavg)
$S capture-pane -p > "$D/results/$PROF-$VIEW-$SCN-$N.screen" 2>/dev/null
CPU=$(python3 -c "print(($K1-$K0)/100/($T1-$T0)*100)")
P=$(python3 $D/parse.py "$COPY/run/tui/$VIEW.log" "$T0" "$T1")
echo "{\"profile\":\"$PROF\",\"view\":\"$VIEW\",\"scn\":\"$SCN\",\"run\":$N,\"cpu\":$CPU,\"ni\":$NI,\"load\":\"$LOAD\",\"win\":$WIN,\"trace\":$P}" | tee -a "$D/results/$PROF.jsonl"
```

The trace parsing (`parse.py`; percentile = sorted index `int(p * n)`; the gaps are all draws of the window with `gap_us` above 0, i.e. the view's very first draw never counts; `skipped` = sum of `(b - a) mod 8 - 1` over consecutive `frame=` values that differ; `draw_us` over all draws of the window):

```python
#!/usr/bin/env python3
# usage: parse.py <trace log> <t0 epoch> <t1 epoch>  -> one JSON object (the draws between t0 and t1)
import sys, re, json, datetime
t0, t1 = float(sys.argv[2]), float(sys.argv[3])
rx = re.compile(r"^(\S+)\s+.*?trace gap_us=(\d+) read_us=(\d+) layout_us=(\d+) draw_us=(\d+) frame=(\d+)")
rows = []
for line in open(sys.argv[1]):
    m = rx.match(line)
    if not m: continue
    t = datetime.datetime.fromisoformat(m.group(1).replace("Z", "+00:00")).timestamp()
    if t0 <= t <= t1: rows.append(tuple([t] + [int(x) for x in m.groups()[1:]]))
def pct(v, p):
    v = sorted(v); return v[min(len(v) - 1, int(p * len(v)))] if v else None
gaps = [r[1] for r in rows if r[1] > 0]       # all draws but the first (gap 0)
frames = [r[5] for r in rows]
skipped = sum(((b - a) % 8) - 1 for a, b in zip(frames, frames[1:]) if b != a)
out = {"draws": len(rows), "gaps": len(gaps),
       "gap_p50_ms": pct(gaps, .5) and pct(gaps, .5) / 1000, "gap_p95_ms": pct(gaps, .95) and pct(gaps, .95) / 1000,
       "gap_max_ms": max(gaps) / 1000 if gaps else None,
       "over150": sum(g > 150000 for g in gaps), "over150_share": sum(g > 150000 for g in gaps) / len(gaps) if gaps else None,
       "frame_changes": sum(b != a for a, b in zip(frames, frames[1:])), "skipped": skipped,
       "draw_p50_ms": pct([r[4] for r in rows], .5) / 1000 if rows else None,
       "draw_p95_ms": pct([r[4] for r in rows], .95) / 1000 if rows else None,
       "read_p50_ms": pct([r[2] for r in rows if r[2]], .5) / 1000 if any(r[2] for r in rows) else None,
       "layout_nonzero": sum(1 for r in rows if r[3])}
print(json.dumps(out))
```

The matrix of one profile (`matrix.sh`), run with `nohup` and polled (about 11 minutes a profile), release first:

```sh
#!/bin/bash
# usage: matrix.sh <binary> <profile>   (3 runs of every scenario but D: 1 run; windows 35 s, D 30 s plan / 35 s tree)
BIN=$1 PROF=$2; D=/tmp/cpd-moon-anim
for s in A1 A2 B; do for n in 1 2 3; do timeout 120 $D/run.sh "$BIN" "$PROF" plan $s $n 35; done; done
timeout 120 $D/run.sh "$BIN" "$PROF" plan D 1 30
for s in A B; do for n in 1 2 3; do timeout 120 $D/run.sh "$BIN" "$PROF" tree $s $n 35; done; done
timeout 120 $D/run.sh "$BIN" "$PROF" tree D 1 35
echo MATRIX-DONE
```

```sh
WT=$(pwd)   # the worktree
/tmp/cpd-moon-anim/matrix.sh $WT/target/release/cpd release > /tmp/cpd-moon-anim/results/release.out
/tmp/cpd-moon-anim/matrix.sh $WT/target/debug/cpd debug > /tmp/cpd-moon-anim/results/debug.out
```

The tables below are made by `table.py` from `results/<profile>.jsonl` (median and min-max of the runs; "gap" columns are the median over runs of each run's p50, p95 and max):

```python
#!/usr/bin/env python3
# usage: table.py <profile>  -> the Before tables (GFM) of results/<profile>.jsonl
import json, statistics as st, sys, collections
prof = sys.argv[1]
rs = [json.loads(l) for l in open("/tmp/cpd-moon-anim/results/%s.jsonl" % prof)]
def f(v, d=1): return "-" if v is None else ("%.*f" % (d, v))
def med(vs, d=1):
    vs = [v for v in vs if v is not None]
    return "-" if not vs else f(st.median(vs), d)
def rng(vs, d=2):
    return "%s-%s" % (f(min(vs), d), f(max(vs), d))
for view in ("plan", "tree"):
    print("\n%s view, %s build:\n" % (view.capitalize(), prof))
    print("| scenario | runs | draws | gap p50 / p95 / max ms | > 150 ms | skipped frames | CPU % median (range) | draw_us p50 / p95 ms | nice | load 1 min |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    g = collections.OrderedDict()
    for r in rs:
        if r["view"] == view: g.setdefault(r["scn"], []).append(r)
    for scn, rr in g.items():
        t = [r["trace"] for r in rr]
        def m(k, d=1): return med([x[k] for x in t], d)
        draws = "%s (%s)" % (med([x["draws"] for x in t], 0), rng([x["draws"] for x in t], 0)) if len(rr) > 1 else str(t[0]["draws"])
        gap = "%s / %s / %s" % (m("gap_p50_ms"), m("gap_p95_ms"), m("gap_max_ms"))
        o150 = "%s (%s %%)" % (med([x["over150"] for x in t], 0), med([100 * x["over150_share"] if x["over150_share"] is not None else None for x in t], 1))
        sk = "%s of %s" % (med([x["skipped"] for x in t], 0), med([x["frame_changes"] for x in t], 0))
        cpu = "%s (%s)" % (med([r["cpu"] for r in rr], 2), rng([r["cpu"] for r in rr]))
        dr = "%s / %s" % (m("draw_p50_ms", 2), m("draw_p95_ms", 2))
        print("| %s | %d | %s | %s | %s | %s | %s | %s | %s | %s |" % (
            ("plan " if view == "plan" else "tree ") + scn, len(rr), draws, gap, o150, sk, cpu, dr,
            "/".join(sorted(set(str(r["ni"]) for r in rr))), med([float(r["load"].split()[0]) for r in rr], 2)))
```

```sh
python3 /tmp/cpd-moon-anim/table.py release
python3 /tmp/cpd-moon-anim/table.py debug
```

Teardown: the trap of `run.sh`; `tmux -L moon-anim-before ls` reported no server afterwards.

### pi

Job moon-before-pi, attempt 1. A REAL interactive pi pane (not the extension's render path only): `node_modules/.bin/pi` of the worktree in a private tmux server, 200x50, loading THIS worktree's `pi/compact-tools/extensions/compact-tools.ts` with `-ne -e <path>` (no other extension, so it is not loaded twice), model `github-copilot/claude-haiku-4.5` (a small model keeps the turn at about 6k tokens), `--offline --no-session`. BASELINE = unchanged code: `git diff --stat $(git merge-base HEAD origin/main)..HEAD -- pi/compact-tools` printed nothing before the runs. The pane runs under `env -i` with only `PATH`, `HOME`, `TERM`, `LANG` and `CPD_HOME` set to an empty temp directory: no `CPD_*` or `PI_*` variable but that one, so no cpd session and no cpd database. The tmux server is `moon-pi-before` and a `trap` kills it and removes the temp directories on any exit.

Scenarios: `work` = one prompt ("Run the bash command: sleep 45. Then answer only: ok"); the script waits for the tool row (`Run sleep`) on screen, then 5 s, then the window is 35 s (so it ends 40 s into the 45 s sleep). Both loaders animate: the working indicator in the input's top border (`╭─<moon> Running command…`) and the running row's closing ` ╰<moon>`. `idle` = the same pane with no turn, 35 s. CPU % is (utime + stime ticks of `/proc/<pid>/stat`, 100 per s) of the pi `node` process (the deepest descendant of the pane whose `comm` is `node` or `pi`) over the window. The pane is captured at the window's start and end (`results/pi-<scn>-<n>.start.screen`, `.end.screen`); `done_at_start` and `done_at_end` count `Done in` lines of the capture (0 = the tool row was still running). The after Job runs the same text (`pi-run.sh`, from the worktree, `work 1..3` and `idle 1`); the tool row is found by the text `Run sleep`, which depends on the model's wording of the command: unverified for other models.

Discarded: a first run with a wrong wait (the pattern `╭─bash` does not match, pi puts private-use glyphs around `bash`) waited 60 s, so the tool had finished; its CPU was 0.00 % and the tool row was `Done` at the window's start. The script below is the fixed one, and every number of the doc is from it.

```sh
#!/bin/bash
# usage: pi-run.sh <work|idle> <run n>   (run from the worktree; WT = the worktree, npm ci done)
# work: one prompt, the agent runs `sleep 45` (bash tool) then answers ok; window 35 s from 5 s into the sleep.
# idle: no turn, 35 s. Prints one JSON line (also appended to results/pi.jsonl).
set -u
SCN=$1 N=$2 WIN=35
WT=$(pwd); D=/tmp/cpd-moon-pi
HOMEDIR=$(mktemp -d /tmp/cpd-moon-pi/cpdhome.XXXXXX)
TT=$(mktemp -d /tmp/cpd-moon-pi/tmux.XXXXXX)
S="env -i PATH=$PATH HOME=$HOME TMUX_TMPDIR=$TT tmux -L moon-pi-before -f /dev/null"
cleanup() { $S kill-server 2>/dev/null; rm -rf "$HOMEDIR" "$TT"; }
trap cleanup EXIT
# no CPD_* / PI_* in the pane except an empty CPD_HOME; --offline = no startup network
$S new-session -d -x 200 -y 50 -c "$WT" env -i PATH="$PATH" HOME="$HOME" TERM=xterm-256color LANG=C.UTF-8 CPD_HOME="$HOMEDIR" \
  "$WT/node_modules/.bin/pi" --offline --no-session -ne -e "$WT/pi/compact-tools/extensions/compact-tools.ts" \
  --model github-copilot/claude-haiku-4.5
sleep 8
PANE=$($S list-panes -F '#{pane_pid}')
# the pi (node) process: the deepest descendant of the pane whose comm is node (or pi)
find_pid() { local p=$1 c; for c in $(pgrep -P "$p"); do find_pid "$c"; done; case "$(cat /proc/$p/comm 2>/dev/null)" in node|pi) echo "$p";; esac; }
PID=$(find_pid "$PANE" | head -1)
[ -z "$PID" ] && { echo "no pi process" >&2; exit 1; }
cap() { $S capture-pane -p; }
if [ "$SCN" = work ]; then
  $S send-keys -l "Run the bash command: sleep 45. Then answer only: ok"
  $S send-keys Enter
  for i in $(seq 1 60); do cap | grep -q "Run sleep" && break; sleep 1; done
  sleep 5
fi
cap > "$D/results/pi-$SCN-$N.start.screen"
ticks() { awk '{print $14+$15}' /proc/$PID/stat; }
T0=$(date +%s.%N); K0=$(ticks)
sleep $WIN
K1=$(ticks); T1=$(date +%s.%N)
NI=$(ps -o ni= -p $PID | tr -d ' ')
LOAD=$(cut -d' ' -f1-3 /proc/loadavg)
cap > "$D/results/pi-$SCN-$N.end.screen"
ROW0=$(grep -c "Done in" "$D/results/pi-$SCN-$N.start.screen"); ROW1=$(grep -c "Done in" "$D/results/pi-$SCN-$N.end.screen")
CPU=$(python3 -c "print(($K1-$K0)/100/($T1-$T0)*100)")
echo "{\"scn\":\"$SCN\",\"run\":$N,\"pid\":$PID,\"cpu\":$CPU,\"ni\":$NI,\"load\":\"$LOAD\",\"win\":$WIN,\"done_at_start\":$ROW0,\"done_at_end\":$ROW1}" | tee -a "$D/results/pi.jsonl"
```

```sh
cd $WT   # the worktree
for n in 1 2 3; do timeout 200 /tmp/cpd-moon-pi/pi-run.sh work $n; done
timeout 100 /tmp/cpd-moon-pi/pi-run.sh idle 1
```

## Before

Columns: runs; draws in the window (median, range); gaps in ms over the draws of the window; share of gaps above 150 ms; skipped frames over frame changes; CPU % (median, range of the runs); `draw_us` p50 / p95 in ms; `ps -o ni` of the view; load average, 1 min at the end of the run (median). A-type scenarios are the median of 3 runs, D is 1 run.

Release build (`target/release/cpd`):

Plan view, release build:

| scenario | runs | draws | gap p50 / p95 / max ms | > 150 ms | skipped frames | CPU % median (range) | draw_us p50 / p95 ms | nice | load 1 min |
|---|---|---|---|---|---|---|---|---|---|
| plan A1 | 3 | 350 (350-350) | 99.9 / 100.9 / 102.1 | 0 (0.0 %) | 0 of 349 | 2.54 (2.54-2.66) | 0.55 / 0.71 | 19 | 4.46 |
| plan A2 | 3 | 350 (350-350) | 99.9 / 100.9 / 101.8 | 0 (0.0 %) | 0 of 349 | 2.46 (2.43-2.51) | 0.53 / 0.64 | 19 | 2.87 |
| plan B | 3 | 350 (350-350) | 99.9 / 101.0 / 102.0 | 0 (0.0 %) | 0 of 349 | 2.57 (2.54-2.57) | 0.53 / 0.75 | 19 | 3.88 |
| plan D | 1 | 0 | - / - / - | 0 (- %) | 0 of 0 | 0.00 (0.00-0.00) | - / - | 19 | 3.08 |

Tree view, release build:

| scenario | runs | draws | gap p50 / p95 / max ms | > 150 ms | skipped frames | CPU % median (range) | draw_us p50 / p95 ms | nice | load 1 min |
|---|---|---|---|---|---|---|---|---|---|
| tree A | 3 | 350 (350-351) | 99.9 / 100.9 / 101.0 | 0 (0.0 %) | 0 of 349 | 0.91 (0.89-0.91) | 0.76 / 1.13 | 19 | 3.18 |
| tree B | 3 | 350 (350-350) | 99.9 / 100.8 / 101.0 | 0 (0.0 %) | 0 of 349 | 0.89 (0.80-0.94) | 0.75 / 1.01 | 19 | 2.70 |
| tree D | 1 | 35 | 1000.0 / 1000.9 / 1001.0 | 35 (100.0 %) | 34 of 34 | 0.11 (0.11-0.11) | 0.76 / 0.93 | 19 | 3.86 |

Load averages at the end of the runs, release: 1 min 2.16-5.39, 5 min 3.17-4.04, 15 min 3.19-3.41 (min-max over the runs).

Debug build (`target/debug/cpd`):

Plan view, debug build:

| scenario | runs | draws | gap p50 / p95 / max ms | > 150 ms | skipped frames | CPU % median (range) | draw_us p50 / p95 ms | nice | load 1 min |
|---|---|---|---|---|---|---|---|---|---|
| plan A1 | 3 | 350 (350-350) | 100.0 / 101.4 / 102.7 | 0 (0.0 %) | 0 of 349 | 4.83 (4.74-5.06) | 1.18 / 1.36 | 19 | 3.20 |
| plan A2 | 3 | 350 (350-350) | 99.9 / 101.7 / 104.5 | 0 (0.0 %) | 0 of 349 | 4.83 (4.74-4.94) | 1.12 / 1.55 | 19 | 2.86 |
| plan B | 3 | 350 (350-350) | 100.0 / 101.8 / 103.7 | 0 (0.0 %) | 0 of 349 | 4.80 (4.37-4.83) | 1.15 / 1.49 | 19 | 3.09 |
| plan D | 1 | 0 | - / - / - | 0 (- %) | 0 of 0 | 0.03 (0.03-0.03) | - / - | 19 | 2.71 |

Tree view, debug build:

| scenario | runs | draws | gap p50 / p95 / max ms | > 150 ms | skipped frames | CPU % median (range) | draw_us p50 / p95 ms | nice | load 1 min |
|---|---|---|---|---|---|---|---|---|---|
| tree A | 3 | 350 (350-351) | 100.2 / 100.5 / 101.6 | 0 (0.0 %) | 0 of 349 | 2.14 (2.00-2.23) | 1.93 / 2.32 | 19 | 2.87 |
| tree B | 3 | 350 (350-350) | 100.2 / 100.4 / 100.9 | 0 (0.0 %) | 0 of 349 | 2.17 (1.89-2.23) | 1.91 / 2.41 | 19 | 2.51 |
| tree D | 1 | 36 | 1000.0 / 1000.5 / 1001.3 | 36 (100.0 %) | 35 of 35 | 0.26 (0.26-0.26) | 1.95 / 2.31 | 19 | 2.42 |

Load averages at the end of the runs, debug: 1 min 2.13-3.83, 5 min 2.77-3.46, 15 min 3.03-3.32 (min-max over the runs).

Notes on the tables: plan D draws nothing in its 30 s window (the one draw is at the start, before the 3 s wait), so its CPU is 0.00 % (release, 0 ticks in 30 s) and 0.03 % (debug, 1 tick); one tick is 0.03 % over 30 s. Tree D redraws once a second by design (the Inbox ages), so its gaps are 1000 ms and its "skipped" is not a stutter measure (every draw is a new frame). The `layout_us` was 0 on every draw of the window in every run (`parse.py` printed `layout_nonzero` 0 in all 34 runs of the table): the layout is not part of the frame cost in this build; `read_us` p50 is 0.001 ms where there is one (a hand-over only). The JSON line of each run is in /tmp/cpd-moon-anim/results/<profile>.jsonl.

### pi

Real pi pane, unchanged code (8 nf-md moons at 100 ms), nice and load as in the JSON line of each run (`results/pi.jsonl`: `ps -o ni` of the pi process; load average, 1 min at the end of the window):

| scenario | runs | CPU % median (range) | nice | load 1 min | what the pane showed |
|---|---|---|---|---|---|
| work (agent working, one `sleep 45` tool row running) | 3 | 4.08 (3.94-4.11) | 19 | 2.72 (2.52-2.92) | start and end of every run: the tool row ` ╭─bash Run sleep` still running, closing row ` ╰<nf-md moon>` and the border pill ` ╭─<nf-md moon> Running command…` with a moon glyph (U+F0F61..F0F68), a different glyph at the start and at the end; no `Done in` line |
| idle (no turn) | 1 | 0.43 (0.43-0.43) | 19 | 2.11 | an empty input box (no pill, no moon), no tool row |

Per run, work: 3.94 %, 4.08 %, 4.11 % (load 2.92, 2.72, 2.52); idle: 0.43 %.


## After

Job moon-after-ui, attempt 1. The changed code: the cockpit with the 28 moon_alt frames at 50 ms (one clock constant for the palette frame, the pump's FRAME read pacing and the pump wake). The method is that of `## Method` / `### Cockpit`, the same scripts run unchanged but for: the binary (`target/release/cpd` and `target/debug/cpd` of this worktree, built with `scripts/cargo.sh build --release -p cpd` (42 s) and `scripts/cargo.sh build -p cpd` (22 s)), the private tmux server name (`moon-anim-after`, in `run.sh`) and the skipped-frames modulus in `parse.py` (`% 28`, the trace's `frame=` runs 0..27). The scripts were re-created from the `### Cockpit` text of this doc (the Before Job's copies in `/tmp/cpd-moon-anim/` were gone; only `base/` was left). Same scenarios, windows (35 s; plan D 30 s), run counts (3 runs of the A-type scenarios, 1 of D), `nice` (19, inherited) and one process at a time.

Checks before the runs:

- Baseline moved: `git diff --stat $(git merge-base HEAD origin/main)..HEAD -- tui pi` lists 19 files (192 insertions, 135 deletions), among them `tui/src/palette.rs`, `tui/src/palette/colours.rs`, `tui/src/pump.rs`, `tui/src/menu.rs`, `tui/src/plan/flow.rs`, `tui/src/widgets/legend.rs`, the tui and pi tests and `pi/compact-tools/extensions/compact-tools-loader.ts`.
- Glyphs: the release binary on a plan view of scenario A2 (private tmux, `tmux capture-pane`, 6 captures 0.37 s apart) showed moon_alt glyphs in the PR title dots (8 cells per capture, all of the same glyph in a capture) and a different glyph in each capture: U+E3DB, U+E3E2, U+E3CE, U+E3D6, U+E3DE, U+E3C9 (all in U+E3C8-U+E3E3); no nf-md moon (U+F0F61-U+F0F68) was on the screen. The pill icon of the running Job was not told apart from the title dots in this check (unverified separately).

Base: `/tmp/cpd-moon-anim/base/`, the SAME base as the Before (its `cpd.db` is dated 2026-10-24 16:30 local, as recorded in `## Method`; not re-made). Every run was on a fresh copy of it.

Columns as in `## Before`. Skipped frames are `(b - a) mod 28 - 1` over consecutive `frame=` values that differ.

Release build (`target/release/cpd`):

| scenario | runs | draws | gap p50 / p95 / max ms | > 150 ms | skipped frames | CPU % median (range) | draw_us p50 / p95 ms | nice | load 1 min |
|---|---|---|---|---|---|---|---|---|---|
| plan A1 | 3 | 700 (700-700) | 49.8 / 51.0 / 52.1 | 0 (0.0 %) | 0 of 699 | 4.88 (4.88-4.94) | 0.52 / 0.62 | 19 | 6.00 |
| plan A2 | 3 | 700 (700-701) | 49.9 / 51.1 / 52.1 | 0 (0.0 %) | 0 of 699 | 5.08 (4.74-5.08) | 0.53 / 0.69 | 19 | 4.92 |
| plan B | 3 | 700 (700-701) | 49.9 / 51.3 / 52.6 | 0 (0.0 %) | 0 of 699 | 5.06 (4.80-5.17) | 0.53 / 0.75 | 19 | 3.11 |
| plan D | 1 | 0 | - / - / - | 0 (- %) | 0 of 0 | 0.03 (0.03-0.03) | - / - | 19 | 3.21 |

Tree view, release build:

| scenario | runs | draws | gap p50 / p95 / max ms | > 150 ms | skipped frames | CPU % median (range) | draw_us p50 / p95 ms | nice | load 1 min |
|---|---|---|---|---|---|---|---|---|---|
| tree A | 3 | 700 (700-700) | 49.9 / 50.9 / 51.0 | 0 (0.0 %) | 0 of 699 | 1.69 (1.63-1.74) | 0.75 / 0.95 | 19 | 2.69 |
| tree B | 3 | 701 (700-701) | 49.9 / 50.9 / 51.0 | 0 (0.0 %) | 0 of 700 | 1.71 (1.60-1.74) | 0.75 / 0.96 | 19 | 2.75 |
| tree D | 1 | 35 | 1000.0 / 1000.9 / 1001.0 | 35 (100.0 %) | 646 of 34 | 0.11 (0.11-0.11) | 0.76 / 1.16 | 19 | 2.36 |

Load averages at the end of the runs, release: 1 min 2.12-6.65, 5 min 3.66-8.92, 15 min 4.81-6.34 (min-max over the 17 runs).

Debug build (`target/debug/cpd`):

Plan view, debug build:

| scenario | runs | draws | gap p50 / p95 / max ms | > 150 ms | skipped frames | CPU % median (range) | draw_us p50 / p95 ms | nice | load 1 min |
|---|---|---|---|---|---|---|---|---|---|
| plan A1 | 3 | 700 (700-700) | 49.9 / 51.7 / 55.0 | 0 (0.0 %) | 0 of 699 | 9.77 (9.17-10.17) | 1.12 / 1.36 | 19 | 3.20 |
| plan A2 | 3 | 701 (700-701) | 50.0 / 52.0 / 61.6 | 0 (0.0 %) | 0 of 700 | 9.91 (9.88-10.11) | 1.15 / 1.59 | 19 | 3.22 |
| plan B | 3 | 701 (700-701) | 49.9 / 52.0 / 56.6 | 0 (0.0 %) | 0 of 700 | 9.74 (9.34-10.03) | 1.11 / 1.58 | 19 | 2.47 |
| plan D | 1 | 0 | - / - / - | 0 (- %) | 0 of 0 | 0.03 (0.03-0.03) | - / - | 19 | 2.81 |

Tree view, debug build:

| scenario | runs | draws | gap p50 / p95 / max ms | > 150 ms | skipped frames | CPU % median (range) | draw_us p50 / p95 ms | nice | load 1 min |
|---|---|---|---|---|---|---|---|---|---|
| tree A | 3 | 700 (700-700) | 50.2 / 50.5 / 51.9 | 0 (0.0 %) | 0 of 699 | 4.31 (4.17-4.34) | 1.93 / 2.69 | 19 | 3.36 |
| tree B | 3 | 701 (700-701) | 50.1 / 50.5 / 52.7 | 0 (0.0 %) | 0 of 700 | 4.14 (4.06-4.14) | 1.90 / 2.56 | 19 | 2.75 |
| tree D | 1 | 35 | 1000.0 / 1000.7 / 1000.8 | 35 (100.0 %) | 646 of 34 | 0.26 (0.26-0.26) | 1.99 / 2.09 | 19 | 3.18 |

Load averages at the end of the runs, debug: 1 min 2.24-4.02, 5 min 2.98-3.75, 15 min 3.85-4.70 (min-max over the 17 runs).

Notes: `layout_nonzero` was 0 in all 34 runs, as before. Tree D "skipped" is `(b - a) mod 28 - 1` over a draw every 1000 ms (20 frames of 50 ms, so 19 skipped per change, 646 of 34): it is the 1 s redraw by design (see the notes under `## Before`), not a stutter. In every run of the A-type scenarios there was no gap over 150 ms and no skipped frame (checked per run), as in the Before. The JSON line of each run is in `/tmp/cpd-moon-anim/results/<profile>.jsonl` (not kept after this Job).

### pi

Job moon-after-pi, attempt 1. The SAME `### pi` script of `## Method`, re-created from this doc (the Before Job's `/tmp/cpd-moon-pi/pi-run.sh` was gone), changed only in the private tmux name (`moon-pi-after`); run from this worktree against THIS worktree's changed `pi/compact-tools/extensions/compact-tools.ts` (28 moon_alt frames at 50 ms; `git diff --stat $(git merge-base HEAD origin/main)..HEAD -- pi/compact-tools` lists `compact-tools-loader.ts`, 32 insertions, 12 deletions). Same prompt, same window (35 s from 5 s after the tool row), same run counts (`work` 1..3, `idle` 1), same nice (19, inherited; the process also inherited an `idle` ionice class), same model, env and size. It is a REAL interactive pi pane (the `node` process under a private tmux server), not the extension's render path only. The server and the temp directories are gone (`tmux -L moon-pi-after ls`: no server).

| scenario | runs | CPU % median (range) | nice | load 1 min | what the pane showed |
|---|---|---|---|---|---|
| work (agent working, one `sleep 45` tool row running) | 3 | 5.26 (5.17-6.23) | 19 | 2.81 (2.71-3.19) | start and end of every run: ` ╭─bash Run sleep` still running, no `Done in` line; the closing row ` ╰<moon>` and the border pill ` ╭─<moon> Running command…` each showed one new-family glyph: pill U+E3E1, U+E3E0 (run 1 start, end), U+E3E2, U+E3DF (run 2), U+E3E0, U+E3DF (run 3); closing row U+E3DB, U+E3DC (run 1), U+E3D7, U+E3D7 (run 2), U+E3D2, U+E3D3 (run 3). All are in U+E3C8..U+E3E3; no nf-md moon (U+F0F61..U+F0F68) in any of the 6 captures. Pill and row showed different glyphs in the same capture (a capture is not one frame of one clock): unverified why |
| idle (no turn) | 1 | 0.34 (0.34-0.34) | 19 | 2.95 | an empty input box, no pill, no moon, no tool row (start and end) |

Per run, work: 6.23 %, 5.26 %, 5.17 % (load 2.81, 3.19, 2.71); idle: 0.34 %.

## Comparison

Before = the `## Before` tables (median of the runs); after = `## After` (median of the runs). Ratio = after / before CPU %; change in points = after - before CPU %. gap and draw_us are the p50 of the run, median over runs. The plan D and tree D rows are the controls (no spinner on screen).

Plan view, release build:

| scenario | CPU % before -> after | ratio | change, points | gap p50 ms before -> after | skipped frames before -> after | draw_us p50 ms before -> after |
|---|---|---|---|---|---|---|
| plan A1 | 2.54 -> 4.88 | 1.92 | +2.34 | 99.9 -> 49.8 | 0 of 349 -> 0 of 699 | 0.55 -> 0.52 |
| plan A2 | 2.46 -> 5.08 | 2.07 | +2.62 | 99.9 -> 49.9 | 0 of 349 -> 0 of 699 | 0.53 -> 0.53 |
| plan B | 2.57 -> 5.06 | 1.97 | +2.49 | 99.9 -> 49.9 | 0 of 349 -> 0 of 699 | 0.53 -> 0.53 |
| plan D | 0.00 -> 0.03 | - | +0.03 | - -> - | 0 of 0 -> 0 of 0 | - -> - |

Tree view, release build:

| scenario | CPU % before -> after | ratio | change, points | gap p50 ms before -> after | skipped frames before -> after | draw_us p50 ms before -> after |
|---|---|---|---|---|---|---|
| tree A | 0.91 -> 1.69 | 1.85 | +0.78 | 99.9 -> 49.9 | 0 of 349 -> 0 of 699 | 0.76 -> 0.75 |
| tree B | 0.89 -> 1.71 | 1.93 | +0.82 | 99.9 -> 49.9 | 0 of 349 -> 0 of 700 | 0.75 -> 0.75 |
| tree D | 0.11 -> 0.11 | 1.04 | +0.00 | 1000.0 -> 1000.0 | 34 of 34 -> 646 of 34 | 0.76 -> 0.76 |

Plan view, debug build:

| scenario | CPU % before -> after | ratio | change, points | gap p50 ms before -> after | skipped frames before -> after | draw_us p50 ms before -> after |
|---|---|---|---|---|---|---|
| plan A1 | 4.83 -> 9.77 | 2.02 | +4.94 | 100.0 -> 49.9 | 0 of 349 -> 0 of 699 | 1.18 -> 1.12 |
| plan A2 | 4.83 -> 9.91 | 2.05 | +5.08 | 99.9 -> 50.0 | 0 of 349 -> 0 of 700 | 1.12 -> 1.15 |
| plan B | 4.80 -> 9.74 | 2.03 | +4.94 | 100.0 -> 49.9 | 0 of 349 -> 0 of 700 | 1.15 -> 1.11 |
| plan D | 0.03 -> 0.03 | 1.11 | +0.00 | - -> - | 0 of 0 -> 0 of 0 | - -> - |

Tree view, debug build:

| scenario | CPU % before -> after | ratio | change, points | gap p50 ms before -> after | skipped frames before -> after | draw_us p50 ms before -> after |
|---|---|---|---|---|---|---|
| tree A | 2.14 -> 4.31 | 2.02 | +2.17 | 100.2 -> 50.2 | 0 of 349 -> 0 of 699 | 1.93 -> 1.93 |
| tree B | 2.17 -> 4.14 | 1.91 | +1.97 | 100.2 -> 50.1 | 0 of 349 -> 0 of 700 | 1.91 -> 1.90 |
| tree D | 0.26 -> 0.26 | 0.99 | -0.00 | 1000.0 -> 1000.0 | 35 of 35 -> 646 of 34 | 1.95 -> 1.99 |

How comparable the two sets are: same base (the same `cpd.db`), same scenario scripts, same windows and run counts, same `nice` 19 in all 34 after runs (and the 34 before runs), one run at a time, tmux server private to the run. The load differed: the load average (1 min, end of run, median of the runs of a scenario) is in the tables; across all runs it was 2.16-5.39 before and 2.12-6.65 after (release), 2.13-3.83 before and 2.24-4.02 after (debug). The machine (16 CPUs) was shared with other Jobs' processes in both sets (the doc records only the load average, not which processes): unverified which Jobs ran in either set. The release after runs of plan A1 had the highest load of the set (median 6.00, vs 4.46 before); the CPU of a scenario did not follow the load order within the after set (plan A1 at the highest load had the lowest plan CPU), which is as far as this doc reads it.

### pi

Pi pane, real, 28 moon_alt frames at 50 ms against the 8 nf-md moons at 100 ms (work: median of 3 runs; idle: 1 run). Ratio = after / before; change in points = after - before CPU %.

| scenario | CPU % before -> after | ratio | change, points | range before -> after | load 1 min before -> after |
|---|---|---|---|---|---|
| work | 4.08 -> 5.26 | 1.29 | +1.18 | 3.94-4.11 -> 5.17-6.23 | 2.72 -> 2.81 |
| idle (control) | 0.43 -> 0.34 | 0.80 | -0.09 | - -> - | 2.11 -> 2.95 |

The 3 work runs after (6.23, 5.26, 5.17) are all above the 3 work runs before (3.94, 4.08, 4.11); run 1 is 1 point above the other two (unverified why). The idle control moved by 0.09 points on 1 run each, which this doc does not separate from noise (one tick over 35 s is 0.03 points). The load was close in the work runs (2.5-2.9 before, 2.7-3.2 after) and not in the idle run.

## Left

- Not measured: nice 0 (the real views and pane), the real cockpit's load: the machine was quiet in these runs (load 1 min about 2-5), far below the load of the earlier docs (7-38), so CPU and gaps are optimistic for the User's real machine; unverified how the cost moves at that load.
- A single machine and a single base: unverified that other hardware gives the same CPU %.
- The snapshot count per run (the trace has no snapshot field): unverified.
- `strace`: not tried (ptrace was denied in the earlier docs): no syscall counts.
- The first-draw cost (time to first paint, 0.7 s in the earlier docs) is outside the windows (the wait is 3 s): not measured.
- Whether the Lead dot and the running Job really spun in every run is read from the trace (draws every 100 ms, 8 frames), not from the screen of each run: unverified for single runs; the screen of each run was captured at its end (not kept here).
- pi: one machine, one model (`claude-haiku-4.5`), one tmux size (200x50); the model's wait for the tool call differs per run (the window starts 5 s after the tool row appears), unverified that the model never streams text inside the window. The nice of the pi pane is 19 (inherited from the Worker session), unverified at nice 0 (the real pane); the load (2-3) is far below the User's usual, so the CPU is optimistic.
- pi: the idle control is 1 run, so its 0.43 % has no range; a node process has no idle cost of zero (timers, the editor's cursor), unverified what it is made of.
- pi: the frame rate of the two loaders is not read from the pane (only two captures per run, a different moon glyph in each); unverified that both really redrew at 100 ms in every run. The CPU is of the node process only, not of the terminal or tmux.
- After (moon-after-ui): the scripts of the Before Job were re-created from this doc, not the original files: unverified that they are byte-identical to the originals; the Before numbers in the Comparison are read from the Before tables of this doc.
- After: whether the loader glyph of the running Job's pill (not only the title dots) changed with the 28 frames is not told apart on the capture (the capture shows the glyph of every spinner cell, all the same); unverified separately. The screen of each run was captured to `results/` and not read; the trace (draws every about 50 ms, `frame=` 0..27) is the evidence that the views animated.
- After: one machine, one base, one run at a time; the load of the after set is not the load of the before set (see `## Comparison`), so a difference of a few tenths of a point between the sets is not separable from load; unverified how the CPU of the changed code moves at the User's usual load (7-38 in the earlier docs) or at nice 0.
- After: the 5 min load reached 8.92 in the release set; unverified which process caused it.
- After (moon-after-pi): the script was re-created from `### pi`, not the original file: unverified that it is byte-identical. The pane's two loaders were seen on 6 captures (2 a run), all new glyphs; their frame rate (50 ms) is not read from the pane, so unverified that both really redrew every 50 ms in every run. The CPU is of the node process only. The `idle` ionice class and nice 19 were inherited from the Worker session in both sets; the idle control is 1 run each.
