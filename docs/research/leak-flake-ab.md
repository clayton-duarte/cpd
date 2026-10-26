# Leak A/B: `test-threads = 4` (main) against `-j 14` (nextest's old default), same pressure, interleaved

Plan leak-flake, Job vdict-docs, attempt a1. Machine `pro` (macOS, arm64, 14 cores), nextest 0.9.148, 2026-10-26 10:00-10:07 EDT, after the Lead's proof ended (`tail -1 /tmp/cpd-leak-flake/proof/summary.txt` = `cleaned: 0 worktrees left`, read 09:34 EDT).

## Verdict

- **4 threads does NOT remove the leak at P2 (6 runs + 4 burners). It cuts it about 4 times.** A (`-j 14`): 34 LEAK-FAIL in 3 rounds (18 runs); B (file's 4): 8 LEAK-FAIL in 3 rounds (18 runs). Verified: tables below, `run*.txt` of each round.
- B leaked in all 3 B rounds (4, 3, 1) at 1-min load 34.6, 39.1 and 49.7 at round start. So the Lead needs another lever, or has to accept a lower rate: 4 is not enough on its own for a leak count of 0. Verified.
- A had one round with 0 (round 5, load 37.3 at start; rounds 1 and 3 gave 20 and 14): the leak is bursty. Neither "B had 0" nor "A had 0 in all 3" held, so P3 was not run (step 3 conditions not met).
- Poisson guess (inference; the counts are over-dispersed, A per round 20/14/0, so this overstates): B's 8 from A's rate of 34 per 3 rounds: P(X<=8) = 1e-7. Conditional binomial (8 of 42 LEAKs in B, p = 0.5 if the settings were equal): 3e-5. Either way, B's rate is lower than A's; not zero.
- Wall time is not worse with 4 threads at this pressure: A 55-61 s, B 59-62 s per run (the machine is the limit). Verified: `wall` files.
- The watcher saw no extra live child at any of the 42 LEAK lines: `pgrep -P <nextest>` showed at most 14 (A) and 4 (B) children, the test slots in use; the leaking test is not listed (unverified which children they were). This agrees with rs-diag: the cause is nextest's timer losing the race, not a process holding the pipe. Verified (children counts), the "no pipe holder" reading is inference from `ps` only.
- One synthetic run (1,800 tests, trivial) is not a Gate (5,100+ tests, 4.8-6.6 ms CPU per exec, 68% system time per rs-cost): inference that a Gate leaks at least as often per test.

## Evidence

Setup (verified: commands in Reproduction). Synthetic workspace as rs-diag (`docs/research/leak-flake-diag.md` `## Reproduction`): package `cpd`, binaries tui, tooling, other, 600 tests each, 1 in 20 CPU-bound; "1800 tests run" in every run. The repo's `.config/nextest.toml` copied UNCHANGED (`test-threads = 4`, `leak-timeout = { period = "100ms", result = "fail" }`). Each round: 6 runs at once, each its own workspace copy and TMPDIR, `-p cpd`, plus 4 `yes > /dev/null` burners killed after the round. A passes `-j 14`; B passes nothing. Order A B A B A B, back to back (rounds ran 10:00:56-10:07:10 EDT). Setting check: max children of nextest seen by the watcher was 14 in A and 4 in B.

Table A, P2, `-j 14` (control):

| round | runs at once | tests/run | LEAK-FAIL (per run) | 1-min load at start | wall per run |
|---|---|---|---|---|---|
| 1 | 6 | 1800 | 20 (5,1,3,3,4,4) | 15.77 | 55-56 s |
| 3 | 6 | 1800 | 14 (3,2,1,0,4,4) | 42.77 | 58-60 s |
| 5 | 6 | 1800 | 0 | 37.33 | 59-61 s |
| total | 6 x 3 = 18 runs | | **34** | | |

Table B, P2, file's 4 threads (treatment):

| round | runs at once | tests/run | LEAK-FAIL (per run) | 1-min load at start | wall per run |
|---|---|---|---|---|---|
| 2 | 6 | 1800 | 4 (2,1,0,0,1,0) | 34.63 | 60-62 s |
| 4 | 6 | 1800 | 3 (0,0,1,1,1,0) | 39.13 | 59 s |
| 6 | 6 | 1800 | 1 (0,0,0,0,0,1) | 49.73 | 61-62 s |
| total | 6 x 3 = 18 runs | | **8** | | |

P3 (8 runs + 6 burners): not run (neither condition of step 3 held: B was not 0, A was not 0 in all rounds). So "how far B holds" has no P3 datum: B already leaks at P2.

Watcher at every LEAK line (per leak: time, children of nextest, full `ps -Ao pid,ppid,etime,pcpu,command`; files `out/<round>/leak<run>.<ns>.ps`): 42 LEAK lines in all (34 A, 8 B). The LEAK-FAIL time in the line was 0.16-0.53 s (B: 0.182, 0.304, 0.161, 0.163, 0.196, 0.526, 0.181, 0.251 s). Children of nextest at the leak (`pgrep -P`): A up to 14, B 3-4, i.e. the slots in use. The per-child `ps -p` lines are empty in all 42 files (the children had gone by the time `ps` ran), so which processes they were is unverified; the leaked test's name is in none of the files (verified: grep). The leaking test names were trivial (`t237`, `t345`, ...). One note (verified): the burners are children of the script, not of nextest.

## Reproduction

Everything in a `mktemp -d`, removed afterwards. `gen.sh`, `burn.sh`, `table.sh` are exactly the ones of the diag file's Reproduction; `round.sh` is the diag file's with three changes: (1) the cargo path comes from `$CARGOSH` (NOT `$CARGO`: `scripts/cargo.sh` reads `$CARGO` as the cargo binary and recurses), (2) the 100 ms only, no scratch config, (3) the header comment. Add `ab.sh`:

```sh
W=$(mktemp -d /tmp/vdab.XXXXXX)            # gen.sh round.sh burn.sh table.sh ab.sh copied in, chmod +x
$W/gen.sh $W/ws; cp <repo>/.config/nextest.toml $W/ws/.config/
export RUSTC_WRAPPER= CARGOSH=<repo>/scripts/cargo.sh
for i in 1 2 3 4 5 6; do cp -R $W/ws $W/ws$i; (cd $W/ws$i && $CARGOSH nextest run -p cpd --no-run); done
$W/ab.sh $W 6 4 p2 A B A B A B       # 6 runs, 4 burners; A = -j 14, B = the file's setting
$W/table.sh $W/out
```
```bash
# round.sh differences against the diag file (diff):
#  -C=$HOME/projects/cpd.leak-flake.gatecost/scripts/cargo.sh      +C=$CARGOSH
#  -CFG=(); [ "$P" != 100ms ] && { sed ... ; CFG=(--config-file ...); }   +CFG=()
#!/bin/bash
# ab.sh <W> <runs> <burners> <tag> <setting...>: A = "-j 14", B = default; one round per setting, in order
W=$1; N=$2; NB=$3; T=$4; shift 4; k=0
for s in "$@"; do k=$((k+1)); B=$($W/burn.sh $NB); J=default; [ $s = A ] && J=14
  timeout 900 $W/round.sh $W $N $J 100ms ${T}${s}_$k | tail -$((N+1)); kill $B; wait $B 2>/dev/null; done
```

## What this means for the fix

- 4 threads is a real gain (about 4 times fewer leaks at the pressure that leaked at 14, no wall-time price at that pressure) but is not a fix: B 8 leaks in 18 runs at load 35-50. Verified. The Plan's expectation that "4 removes it" is refuted at P2. Caveat: one synthetic run of 1,800 trivial tests; a Gate is about 3 times longer, so each Gate has more chances to lose the race (inference).
- 24 test processes + 4 burners (B) is below the 42 that was clean in real Gates (rs-dyn) and still leaked here, so "G x 4 <= 42" is not a clean-cell guarantee on this synthetic load; the load average (35-50) and the burners count, not only test processes (inference; fits rs-diag `-j 1` at load 174). The Worker cap of 10 supported by 10 x 4 = 40 is therefore unproven by these numbers.
- Levers left (all inference, not run here): fewer threads (`test-threads` 2-3; rs-dyn says T = 3 allows 14 Gates), a lower Worker cap, or the leak-timeout (forbidden by the brief: the User's to weigh; rs-diag found 0 at 1 s and 5 s). A next Job could run B at 2 and 3 threads, same P2, interleaved, 3 rounds each (about 15 minutes).
- Cost of this Job: nothing in the repo changed except this file.
