# Leak levers: nice / QoS wrapper and leak period 200 ms / 1 s, A/B at P2

Plan leak-flake, Job lever-docs, attempt a1. Machine `pro` (macOS 26.7.1 arm64, 14 cores, cargo-nextest 0.9.148), 2026-10-26 10:32-11:36 EDT. Synthetic workspace and harness of `docs/research/leak-flake-ab.md` (`## Reproduction`). The 200 ms, 1 s and wrapper configs are DIAGNOSTIC scratch configs outside the repo; nothing in the repo's `.config/nextest.toml` changed.

## Verdict

- **No LEAK-FAIL in any arm, including the control B: 0 in 26 rounds (156 runs; B alone 0 in 6 rounds, 36 runs).** The leak did not reproduce today, so this A/B cannot rank the levers (verified: `grep -h "leaky\|LEAK" run*.txt` of every kept round prints nothing; table below). The extension rule (an arm at 0 while B has >= 3) did not apply: B had 0.
- Why B did not leak is open. The earlier A/B (ab.md) gave B 8 in 18 runs at the same recipe; today 0 in 36 (Poisson guess, not a measure: P(0 | mean 16) ~ 1e-7, so the recipe or the machine differed, not luck). Candidate differences (inference, not tested): (1) the whole harness ran under launchd at nice 0 (the Worker shell is nice 19, so the A/B ran at nice 19 with its burners) because N needs nextest at 0; (2) I dropped the per-LEAK `ps` watcher (it added load); (3) the other Plans' load of the day (1-min load at round start 8.6-74; other nextest runs at round start: 0 in P2 rounds, 1 in rounds 4-5 half the time).
- **N works as a mechanism (verified): nextest nice 0, test processes nice 10** (`ps`, 12 samples, below). Effect on speed is clear: wall per run 124 s against 76 s for B at P2 (+63%). Whether it lowers the leak is unmeasured (B had none).
- **N2 (`taskpolicy -c utility`) exists and runs but costs the most**: 124 s at P2 (same as N), 266 s median and 492 s max at P2+ (B 80/156). Its effect on priority is not visible in `ps` nice (stays 0); verified only through the wall time.
- L2 (200 ms) and L1 (1 s) cost nothing measurable in wall (P2: 82 and 79 s against B 76 s; same machine state, differences within the day's load drift). They cannot be ranked against B by leaks.
- A real leak (ms-leak, 2.4-3.0 s, rs-hs) is still caught by 100 ms, 200 ms and 1 s (verified by arithmetic on that range: all periods are below 2.4 s); a leak between 0.1 and 1 s is not known either way.
- Limits: 5-6 rounds per arm, synthetic, one machine, no leak seen: the evidence favours none; it only shows N and N2 slow the run and L2/L1 do not.

## Evidence

Arms (all `test-threads = 4` from the file, no `-j`; one round = 6 runs at once, each its own workspace copy and TMPDIR, `-p cpd`, 1800 tests per run, plus `yes > /dev/null` burners killed after it). Rounds 1-3 = P2 (4 burners), interleaved in the order B N N2 L2 L1, 3 times. Rounds 4-5 = P2+ (10 burners), the same order twice: added because P2 gave no leak at all (not in the spec; the "extension" in the Job's text needed B >= 3). Plus one extra B round (`rerun_B`), see "Incident".

| arm | rounds (P2 + P2+) | runs | LEAK-FAIL | 1-min load at round start | wall per run (median, max) P2 / P2+ | other nextest procs at round start |
|---|---|---|---|---|---|---|
| B (4 thr, 100 ms) | 3 + 2 + 1 rerun = 6 | 36 | 0 | P2 8.64, 35.55, 32.79, rerun 33.92; P2+ 13.56, 64.30 | 76 s, 91 s / 80 s, 156 s | P2 0 (round 1 not recorded); P2+ 0, 1 |
| N (nice 10 wrapper) | 3 + 2 = 5 | 30 | 0 | P2 29.65, 34.94, 34.62; P2+ 34.84, 59.14 | 124 s, 141 s / 117 s, 189 s | P2 0 (round 1 not recorded); P2+ 0, 1 |
| N2 (`taskpolicy -c utility`) | 3 + 2 = 5 | 30 | 0 | P2 38.99, 26.60, 26.58; P2+ 39.93, 54.30 | 124 s, 140 s / 266 s, 492 s | P2 0 (round 1 not recorded); P2+ 0, 0 |
| L2 (200 ms) | 3 + 2 = 5 | 30 | 0 | P2 28.34, 29.31, 27.03; P2+ 73.72, 68.98 | 82 s, 87 s / 152 s, 271 s | P2 0 (round 1 not recorded); P2+ 0, 1 |
| L1 (1 s) | 3 + 2 = 5 | 30 | 0 | P2 33.61, 32.84, 35.57; P2+ 61.41, 66.84 | 79 s, 83 s / 112 s, 168 s | P2 0 (round 1 not recorded); P2+ 1, 1 |

Verified by `table.sh out` (per round: runs, leaks, load, others, wall per run) and the per-run `run*.txt` / `wall*`. The P2+ walls rise with load (rounds 4-5 had load 54-74 at start in 5 of 10 rounds: the other Plans' work), so compare arms within a round pair only: B 80/156, N 117/189, L1 112/168, L2 152/271 are not in the same load.
Chance that an arm's 0 came from B's rate: B's own rate today is 0 of 36, so the question is empty; against the earlier A/B rate (8 in 18 runs) an arm's 0 in 30 runs would be P ~ 1e-6 (Poisson guess; the counts are bursty, so it overstates), but then B's 0 refutes that rate for today (guess).

**Wrapper check (verified):** a run of the N arm under launchd with `ps -Ao pid,ppid,nice,command` sampled 6 times: `cargo-nextest nextest run -p cpd` NI 0; test processes (`.../deps/other-... --exact t320 --nocapture`, t340 ... t420) NI 10 in all samples. The wrapper `sh` before `exec` shows NI 0 as `cargo-nextest __double-spawn -- nice10.sh ...` (nextest's own helper, then `nice -n 10`). The syntax in the scratch config is the one of rs-cpd: `experimental = ["wrapper-scripts"]` first line, `[scripts.wrapper.lo] command = '<abs path>'`, `[[profile.default.scripts]] filter = 'all()'` `run-wrapper = 'lo'`: it ran on 0.9.148 unchanged (verified).
**N2 check:** `man taskpolicy` on this machine: `-c clamp` "Run the program using the specified QoS clamp. The argument can be either utility, background, or maintenance"; children inherit. The flag exists (verified: the arm ran). `ps -o nice` stays 0 for it (verified: 6 samples), so only the wall time shows it works. A `ps -o pri` on `yes`: plain 20, nice 10 20, `-c utility` 20, `-c background` 4 (verified once): utility is not visible in `pri` either.
**Nice caveat (verified):** the Worker's shell is nice 19 (`ps -o nice= -p $$` = 19) and a process cannot lower its nice without root, so a wrapper started from it would give nextest 19 and tests 20 (1 step). The arms ran under `launchctl submit` (nice 0) for this reason. A Gate started by the cpd engine may run at another nice: not checked (unverified).

**Idle price (verified: `idle.sh`, 20 runs of `other-... --exact t001`, 3 interleaved passes, machine load about 20-28, so not really idle):** plain 9.3 / 7.0 / 9.3 ms per run; `nice -n 10` 15.7 / 13.6 / 15.8; `taskpolicy -c utility` 16.4 / 14.1 / 11.2. About +6 ms per test is the extra exec of `nice` or `taskpolicy` (inference: a second process image, not the priority itself). On 5,100 tests / 4 threads: about 5,100 x 6 ms / 4 = 8 s more CPU-time wall in an idle Gate (guess; arithmetic). Under load the price is the wall above: +63% at P2 for N and N2.
**L2/L1 price:** nothing in wall (P2 82 s / 79 s against B 76 s). What each period still catches: `ms-leak` real leaks lasted 2.4-3.0 s (`leak-flake-hs.md` line 5): both periods catch them; a leak that closes its pipes between 100 ms and the period is missed: none is known (hist, verdict (i): inference).
**Incident (verified):** `launchctl submit` restarts a job that ended unless the job removes itself: my first launch re-ran the 15 rounds from the start at 10:58 and overwrote `r1_1_B` and `r1_2_N` before I removed it. The original round 1 numbers (B load 8.64, 0 leaks, wall 90-91 s; N load 29.65, 0, 139-141 s) are from my own earlier `table.sh` output; the overwritten `r1_1_B` is kept as `rerun_B` (load 33.92, 0 leaks, 76 s), `r1_2_N` partial was deleted. `launch.sh` below now removes the job itself. Other tooling fixes: macOS `pgrep` has no `-c` (round 1-3 "others" blank), `sed -i` needs `''`.
Not recorded: the per-LEAK `ps` watcher of the A/B (no leak happened to trigger it).

## Reproduction

`gen.sh`, `burn.sh` and `table.sh` are the diag file's (`leak-flake-diag.md` `## Reproduction`), unchanged. Build 6 copies as in ab.md, copy the repo's `.config/nextest.toml` into `ws/.config`, `export CARGOSH=<repo>/scripts/cargo.sh`, then:

```sh
base=$W/ws/.config/nextest.toml
mk(){ { echo 'experimental = ["wrapper-scripts"]'; cat $base; printf "\n[scripts.wrapper.lo]\ncommand = '$2'\n[[profile.default.scripts]]\nfilter = 'all()'\nrun-wrapper = 'lo'\n"; } > $1; }
mk $W/scratch-N.toml $W/nice10.sh; mk $W/scratch-N2.toml $W/util.sh
sed 's/period = "100ms"/period = "200ms"/' $base > $W/scratch-L2.toml
sed 's/period = "100ms"/period = "1s"/'   $base > $W/scratch-L1.toml
# nice10.sh: #!/bin/sh + exec nice -n 10 "$@"      util.sh: #!/bin/sh + exec taskpolicy -c utility "$@"   (chmod +x)
```
```bash
#!/bin/bash
# round.sh <W> <runs> <arm: B|N|N2|L2|L1> <tag>   (B = the copy's own file; others = scratch --config-file)
W=$1; N=$2; A=$3; TAG=$4; O=$W/out/$TAG; mkdir -p $O
CFG=(); [ $A != B ] && CFG=(--config-file $W/scratch-$A.toml)
echo "$TAG load: $(uptime | sed 's/.*load averages: //') others: $(pgrep -f 'cargo-nextest nextest run' | wc -l | tr -d ' ')" > $O/head
for i in $(seq $N); do ( cd $W/ws$i; mkdir -p $W/tmp$i; s=$(date +%s)
  TMPDIR=$W/tmp$i RUSTC_WRAPPER= $CARGOSH nextest run -p cpd "${CFG[@]}" > $O/run$i.txt 2>&1
  echo "wall $(( $(date +%s) - s ))" > $O/wall$i ) & done; wait
for i in $(seq $N); do echo "run$i $(grep -o '[0-9]* due to being leaky' $O/run$i.txt | cut -d' ' -f1 | head -1 | grep . || echo 0) leaks $(grep -o '[0-9]* tests run' $O/run$i.txt | head -1) $(cat $O/wall$i)s"; done >> $O/head
```
```bash
#!/bin/bash
# arms.sh <W> <runs> <burners> <arm...>: one round per arm, in order (RND = repetition number in the tag)
W=$1; N=$2; NB=$3; shift 3; k=0; export PATH=/opt/homebrew/bin:$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin
for a in "$@"; do k=$((k+1)); B=$($W/burn.sh $NB); timeout 600 $W/round.sh $W $N $a r${RND:-1}_${k}_$a; kill $B; wait $B 2>/dev/null; done
# P2: for r in 1 2 3; do RND=$r arms.sh $W 6 4 B N N2 L2 L1; done      P2+: RND=4/5 with 10 burners
```
```sh
#!/bin/sh
# launch.sh <label> <cmd...>: run under launchd (nice 0); the job removes itself, else launchd restarts it. W and CARGOSH exported.
l=$1; shift; launchctl submit -l $l -o $W/$l.out -e $W/$l.err -- /bin/sh -c 'env CARGOSH='$CARGOSH' HOME='$HOME' "$@"; launchctl remove '$l -- sh "$@"
# nice check: run one nextest with scratch-N.toml, sample `ps -Ao pid,ppid,nice,command | grep "$W/ws1/target\|-p cpd"` 6 times
```

## What this means for the fix

Proposals only (the file is not edited; each needs the User's decision, and the brief's "never raise leak-timeout" is lifted for measuring only). None showed a LEAK-FAIL reduction here because B had none today, so none is proven.

- **N (nice):** top of `.config/nextest.toml`: `experimental = ["wrapper-scripts"]`; then `[scripts.wrapper.lo]` with `command = 'scripts/nice10.sh'` (a new script; check that a relative path works: unverified, the arms used an absolute path), and `[[profile.default.scripts]]` with `filter = 'all()'` and `run-wrapper = 'lo'`. Price: +63% wall at P2 (124 s against 76 s), about +6 ms per test idle; experimental feature of nextest; works only when nextest itself starts at a lower nice than 10 above its parent (a Worker shell at 19 gets 19/20, no gain: unverified how the Gate runs).
- **N2:** the same with `command = 'scripts/utility.sh'` (`exec taskpolicy -c utility "$@"`); price +63% at P2 and up to 3.3 times (266 s against 80 s) at P2+: worse than N, macOS only (the Gate on `pro` is macOS; CI is not).
- **L2:** change line 35 to `leak-timeout = { period = "200ms", result = "fail" }` (nextest's default period; rs-hist). No wall price. Real 2.4-3.0 s leaks still caught.
- **L1:** `leak-timeout = { period = "1s", result = "fail" }`. No wall price; rs-diag: 0 at 1 s in 3 rounds where 100 ms gave 23. Real 2.4-3.0 s leaks still caught; a leak under 1 s that clears itself is not.
- **Not covered by any of them:** a leak at a load far above P2 (`-j 1` at load 174 leaked at 100 ms in rs-diag; no arm was tried there); the cause (nextest's timer losing the race) is untouched by N/N2 unless nextest really runs earlier than the burners and other Plans, which was not shown (nextest at nice 0 competes with the burners at nice 0). A next measurement needs a pressure where B leaks (run B alone first, e.g. at the A/B's nice 19 with the watcher, until 3 leaks per 3 rounds, then the arms).
- The evidence so far favours none by leaks; by price L2 and L1 cost nothing and N/N2 cost a lot (verified), so if the User wants a lever on the cheaper side the data supports only that sentence.
