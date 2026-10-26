# Leak flake diagnostic: does a longer leak period remove the LEAK (H3a)? (rs-diag, attempt 3)

## Verdict

- **H3a (no holder, nextest's 100 ms timer loses a race under load): CONFIRMED** for nextest 0.9.148 on this machine, on a synthetic workspace of 1,800 trivial tests per run that spawn nothing. 100 ms, default threads, 6 runs + 4 burners: 23 LEAK-FAIL in 3 rounds. DIAGNOSTIC 1 s and 5 s period, same setting, load 54-113: 0 LEAK in 3 rounds each (verified: tables below).
- No extra holder: 30 watcher snapshots taken at the LEAK line, 0 had a live child of that nextest that was not a running test (`__double-spawn ... --exact tNNN`), and none was the leaked test itself (verified: `out/*/leak*.ps`, analysis loop in Evidence).
- At `-j 64` (the 100 ms rate is lower: 5 LEAK in 6 rounds with burners, 1 in 3 without) the 1 s and 5 s rounds also had 0 in 3 rounds each: consistent with H3a, but weak (guess: about 8% chance of 0 in 3 rounds if the 100 ms rate were unchanged, Poisson): "consistent", not proof.
- `-j 1` at 100 ms still LEAKed (4 in 1 round, at most one test process at a time, the snapshots show no other child): the race is not about many test processes alone. No 1 s `-j 1` round was run.
- H1 (std pipe window): not tested here; killed for nextest by rs-src. H2 (a test's own child): not applicable, no test spawns. H3c (OS/scanners delay nextest's threads): consistent (leaks only at load 50-175, 0 at load 20-30 with 4 runs), NOT isolated: this Job did not look at which OS component stalls. H3b: open, nothing found.
- No leak at all in 3 rounds of 4 runs, load 20-24, default threads (0 of 3). The leak needs load.

## Evidence

Machine: macOS 26.7.1 (verified `sw_vers`), arm64, 14 cores (verified `sysctl hw.ncpu`), other users' Gates and the endpoint products running (load 20-175 at the round starts). cargo-nextest 0.9.148. Workspace: package `cpd` with 3 test binaries (`tui`, `tooling`, `other`) of 600 tests each = 1,800 tests per run; the unchanged repo `.config/nextest.toml` (its `binary_id(cpd::tui)` filters fail to parse if the binary ids do not exist, so the package and binaries carry those names; the tests are `t000..t599`, 5% a 300-600 ms CPU loop in a debug build, no spawn, sleep or file). Each concurrent run has its own copy of the workspace (`ws1..ws6`, built once with `--no-run`) and its own TMPDIR. `nextest run --workspace` is refused by the Worker adapter even in the synthetic workspace, so `-p cpd` was used (all three binaries). Pressure P1 = 4 runs, no burners. Pressure P2 = 6 runs at once (the maximum allowed) + 4 `yes > /dev/null` burners, killed after each round. "Load" is the 1-minute load average at the round start (it includes the previous round). "Wall" is the run wall time (min-max over the runs of the round). Every round: 1,800 tests per run, all passed except the LEAK-FAIL ones.

Table row: round | concurrent runs | LEAK (all runs) | load | wall s.

### 100 ms (the repo's config, unchanged), default threads (14)

| round | runs | LEAK | load | wall |
|---|---|---|---|---|
| t1 (P1) | 4 | 0 | 23.7 | 34-35 |
| d4_2 (P1) | 4 | 0 | 20.5 | 32-33 |
| d4_3 (P1) | 4 | 0 | 21.4 | 33-33 |
| p2d_2 (P2) | 6 | 6 | 68.2 | 67-68 |
| p2dz_1 (P2) | 6 | 5 | 49.5 | 58-59 |
| p2dz_2 (P2) | 6 | 12 | 54.4 | 55-59 |

### 100 ms, `-j 64`

| round | runs | LEAK | load | wall |
|---|---|---|---|---|
| j64_4_1 (P1) | 4 | 0 | 29.1 | 33-33 |
| j64_4_2 (P1) | 4 | 1 | 46.3 | 35-36 |
| j64_4_3 (P1) | 4 | 0 | 42.9 | 33-33 |
| p2j64_1 (P2) | 6 | 0 | 42.4 | 49-50 |
| p2j64_2 (P2) | 6 | 2 | 85.5 | 50-51 |
| p2j64x_1 (P2) | 6 | 0 | 68.0 | 54-55 |
| p2j64y_1 (P2) | 6 | 1 | 158.8 | 57-58 |
| p2j64y_2 (P2) | 6 | 0 | 109.2 | 55-56 |
| p2j64y_3 (P2) | 6 | 2 | 106.7 | 55-55 |

### DIAGNOSTIC (scratch `--config-file` outside the repo, `period` changed, nothing in the repo changed): 1 s and 5 s, P2

| setting | round | runs | LEAK | load | wall |
|---|---|---|---|---|---|
| 1 s, default | p2d1s_1 / _2 / _3 | 6 | 0 / 0 / 0 | 112.8 / 81.4 / 68.5 | 53-54 / 72-73 / 54-56 |
| 5 s, default | p2d5s_1 / _2 / _3 | 6 | 0 / 0 / 0 | 70.6 / 56.2 / 54.2 | 64-65 / 54-57 / 57-60 |
| 1 s, `-j 64` | p2j1s_1 / _2 / _3 | 6 | 0 / 0 / 0 | 53.2 / 74.1 / 88.4 | 51-52 / 54-55 / 52-53 |
| 5 s, `-j 64` | p2j5s_1 / _2 / _3 | 6 | 0 / 0 / 0 | 100.2 / 111.6 / 110.8 | 51-51 / 52-53 / 54-55 |

### `-j 1`, 100 ms (the repo's config), P2

| round | runs | LEAK | load | wall |
|---|---|---|---|---|
| p2j1_1 | 6 | 4 (runs 1, 4, 6: 1, 1, 2) | 174.5 | 116-119 |

### What the watcher saw at each LEAK line

The watcher (in `round.sh`) tails the run's output and, at the first `LEAK-FAIL` line while that nextest is alive, writes `pgrep -P <nextest>` with `ps` for each child, then `ps -Ao pid,ppid,etime,pcpu,command`. 30 snapshots for 33 LEAK lines (3 LEAK lines had no snapshot: the watcher lost them; one extra snapshot, `j64_4_2/leak1.1792989962954422000.ps`, is the final-summary reprint of a LEAK line, with 0 children, ignored). Analysis, per snapshot: children listed that are not `--exact tNNN` of that run's own workspace = "extra"; children with the leaked test's name = "same test alive".

| setting | LEAK lines | snapshots | extra live child | leaked test's own process alive | children of that nextest at the report (`pgrep -P`) |
|---|---|---|---|---|---|
| 100 ms default P2 | 23 | 21 | 0 | 0 | 13-14 (the 14 test slots), 1-12 still alive at `ps` time |
| 100 ms `-j 64` P1+P2 | 6 | 6 | 0 | 0 | 62-64 |
| 100 ms `-j 1` P2 | 4 | 3 | 0 | 0 | 1 (the next test; already gone at `ps`) |

Every child alive was a `cargo-nextest __double-spawn -- .../tests-binary --exact tNNN --nocapture` of the same run: a running test. "Alive at `ps` time" is lower than the `pgrep` count because 1 ms tests exit between the two commands. UNVERIFIED: that each snapshot was taken within 50 ms of the line (the script records the time of the snapshot, not of the line; `ps -Ao` of ~500 processes under this load can take longer). A holder that exited in that time would be missed; a holder that stays alive for 1 s would not (the 1 s and 5 s rounds have 0 LEAK, which is the stronger evidence).

What could not be seen: no `fs_usage`, `dtruss`, `spindump` or `eslogger` (sudo needs a password): the OS component that delays nextest was not identified.

## Reproduction

Smallest recipe: generate the workspace, build 6 copies, run `batch.sh` at 100 ms, then at 1 s (the DIAGNOSTIC scratch config is written by `round.sh`). Everything lives in a `mktemp -d` outside the repo; remove it afterwards. `W` is that directory; the repo's `.config/nextest.toml` is copied to `$W/ws/.config/` first.

```sh
W=$(mktemp -d /tmp/rsdiag.XXXXXX); cp gen.sh round.sh burn.sh batch.sh table.sh $W/   # the scripts below
$W/gen.sh $W/ws; cp <repo>/.config/nextest.toml $W/ws/.config/
export RUSTC_WRAPPER=; for i in 1 2 3 4 5 6; do cp -R $W/ws $W/ws$i; (cd $W/ws$i && <repo>/scripts/cargo.sh nextest run -p cpd --no-run); done
$W/batch.sh $W 3 default 100ms p2d      # 6 runs + 4 burners, 3 rounds: LEAKs expected
$W/batch.sh $W 3 default 1s    p2d1s    # DIAGNOSTIC: expect 0
$W/table.sh $W/out
```

gen.sh:
```sh
#!/bin/sh
# gen.sh <dir>: package `cpd` with 3 test binaries (tui, tooling, other) x 600 trivial tests,
# 5% CPU-bound. Named so the repo's unchanged nextest.toml filters (binary_id(cpd::tui)) parse.
d=$1; mkdir -p $d/.config $d/src $d/tests
printf '[package]\nname="cpd"\nversion="0.0.0"\nedition="2021"\n[workspace]\n' > $d/Cargo.toml
: > $d/src/lib.rs
for c in tui tooling other; do
  awk 'BEGIN{ for(i=0;i<600;i++){
    if(i%20==0) n=30000000+(i%7)*5000000; else n=20000;
    printf "#[test]\nfn t%03d() {\n let mut x: u64 = %d;\n for i in 0..%d_u64 { x = x.wrapping_mul(6364136223846793005).wrapping_add(i); }\n assert!(std::hint::black_box(x) != 1);\n}\n", i, i+1, n } }' > $d/tests/$c.rs
done
```

round.sh:
```bash
#!/bin/bash
# round.sh <W> <runs> <threads|default> <period: 100ms|1s|5s> <tag>
# runs <runs> nextest runs at once, each in its own copy of the workspace ($W/wsN) and TMPDIR.
# 100ms uses the workspace's own unchanged .config/nextest.toml; others a scratch --config-file (DIAGNOSTIC).
W=$1; N=$2; J=$3; P=$4; TAG=$5; export RUSTC_WRAPPER=
C=$HOME/projects/cpd.leak-flake.gatecost/scripts/cargo.sh
O=$W/out/$TAG; mkdir -p $O
echo "$TAG load: $(uptime | sed 's/.*load averages: //')" > $O/head
CFG=(); [ "$P" != 100ms ] && { sed "s/period = \"100ms\", result = \"fail\"/period = \"$P\", result = \"fail\"/" $W/ws/.config/nextest.toml > $W/scratch-$P.toml; CFG=(--config-file $W/scratch-$P.toml); }
JJ=(); [ "$J" != default ] && JJ=(-j $J)
for i in $(seq $N); do (
  cd $W/ws$i; mkdir -p $W/tmp$i; s=$(date +%s)
  TMPDIR=$W/tmp$i $C nextest run -p cpd "${CFG[@]}" "${JJ[@]}" > $O/run$i.txt 2>&1 & NP=$!
  tail -n +1 -f $O/run$i.txt 2>/dev/null | while IFS= read -r l; do
    case "$l" in DONE*) break;; *Summary*) sum=1;; *LEAK*cpd::*) [ -n "$sum" ] && continue; # first sighting while nextest alive
      kill -0 $NP 2>/dev/null || continue
      t=$(echo "$l" | awk '{print $NF}')
      { echo "== $l"; date +%T.%N; echo "children: $(pgrep -P $NP | wc -l)"
        for c in $(pgrep -P $NP); do ps -o pid=,etime=,state=,command= -p $c; done
        ps -Ao pid,ppid,etime,pcpu,command; } > $O/leak$i.$(date +%s%N).ps;; esac
  done &
  wait $NP; echo "wall $(( $(date +%s) - s ))s" > $O/wall$i; echo "DONE" >> $O/run$i.txt; sleep 0.3; pkill -f "tail -n .1 -f $O/run$i.txt"; wait
) & done; wait
for i in $(seq $N); do echo "run$i $(grep -o "[0-9]* due to being leaky" $O/run$i.txt | cut -d" " -f1 | head -1 | grep . || echo 0) leaks $(grep -o '[0-9]* tests run' $O/run$i.txt|head -1) $(cat $O/wall$i)"; done >> $O/head
cat $O/head
```

burn.sh, batch.sh, table.sh:
```bash
#!/bin/sh
# burn.sh <n>: n CPU burners; prints their pids (kill them afterwards)
for i in $(seq $1); do yes > /dev/null & echo $!; done
#!/bin/bash
# batch.sh <W> <rounds> <threads> <period> <tag>: <rounds> rounds of 6 runs + 4 `yes` burners
for r in $(seq $2); do B=$($1/burn.sh 4); timeout 400 $1/round.sh $1 6 $3 $4 $5_$r | tail -7; kill $B; done
#!/bin/bash
# table.sh <out dir>: one line per round: tag runs leaks load wall(s)
for d in $1/*/; do t=$(basename $d); n=$(ls $d/run*.txt | wc -l); l=0
 for f in $d/run*.txt; do x=$(grep -o "[0-9]* due to being leaky" $f | cut -d' ' -f1 | head -1); l=$((l+${x:-0})); done
 w=$(cat $d/wall* | sed 's/[^0-9]//g' | sort -n | sed -n '1p;$p' | paste -sd- -)
 echo "$t | $n | $l | $(sed -n 1p $d/head | sed 's/.*load: //' | cut -d' ' -f1) | $w"; done
```

## What this means for the fix

- The defect is not a held pipe: with a 1 s or 5 s period the same runs under the same load give 0 LEAK, and at the 100 ms LEAK no extra process is alive. So the `leak-timeout = 100ms` window is shorter than nextest's own scheduling jitter at load 50+ (the timer starts after `child.wait()` returns and EOF is read by the same task: rs-src `executor.rs:1487`). The same symptom as nextest#1469 (open, undiagnosed): this recipe is a candidate reproduction for it, no cpd code involved.
- Options (no change made here; the Manager's rules forbid raising `leak-timeout`, `result = "pass"`, retries and quarantine in the repo): (a) report upstream with the recipe and ask for a leak check that waits for EOF on the pipe independent of the timer (cost: a day of the Lead, outcome not ours); (b) lower the load a Gate causes (fewer concurrent test processes: `-j` below the default does NOT help, `-j 1` leaked; fewer concurrent Gates is the user's decision pro-d30, no slots); (c) a longer period removes the LEAK here (0 at 1 s and 5 s) but it is the option the Manager rejected; only the Manager can reopen it.
- Not known: what stalls nextest (scanners, Spotlight, kernel): needs privileged tools or the User's machine view. The `-j 1` result suggests the stall is in nextest's process or the OS, not the number of tests.
