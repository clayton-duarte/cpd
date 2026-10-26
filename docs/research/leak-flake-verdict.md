# Verdict of Plan leak-flake: why a Gate gets LEAK-FAIL on tests that spawn nothing, and what is fixed

Machine `pro` (macOS 26.7.1, arm64, 14 cores), cargo-nextest 0.9.148, written 2026-10-26 10:10 EDT. Sources are the findings files `docs/research/leak-flake-<x>.md`, named `rs-<x>` (src, cpd, os, cost, diag, hist, hs, dyn, std, ab) with the section cited. Labels: `verified` (a file line or command shows it), `inference`, `unverified`. Prose times are EDT; the files' own times are as they wrote them.

## Verdict

- **Cause (rs-diag Verdict, verified on a synthetic workspace):** no process holds the pipe; nextest's `leak-timeout` timer (100 ms, started when `child.wait()` returns: rs-src Evidence 1.3) expires before nextest, starved when the machine is loaded, reads the EOF. 1,800 trivial tests per run, 6 runs + 4 CPU burners: 23 LEAK-FAIL in 3 rounds at 100 ms, 0 at 1 s and at 5 s (3 rounds each); 30 snapshots at the LEAK line showed no extra child.
- **Done:** `test-threads = 4` in `.config/nextest.toml:30` (PR threads #461, verified by reading the file); `leak-timeout` unchanged (`:35`).
- **Is it fixed? No: cut, not removed.** A/B (rs-ab Verdict): same pressure, 18 runs each, `-j 14` gave 34 LEAK-FAIL, the file's 4 threads gave 8 and leaked in all 3 rounds. Real Gates: 6 of 6 clean in the Lead's proof, but 3 Gates x 4 = 12 processes, where 14 threads were already clean (inference, Proof a); and 1 of the 9 Gates since the change (01:09 EDT) had 3 LEAK-FAIL (Proof c). The after-count is weak evidence (load differed).
- **What the machine supports:** a Worker cap of 10 at 4 threads by the rule G x T <= 42 (10 x 4 = 40); 14 needs 3 threads (rs-dyn Verdict). But rs-ab found 24 processes + 4 burners already leaked, so "<= 42 is clean" is no guarantee: the cap of 10 is unproven by the numbers (rs-ab "What this means").
- **Still open (the User's):** the leak period (1 s and 5 s removed every LEAK; the brief forbids raising it), an upstream nextest issue, the Worker cap (10 or 14), the homeserver experiment, an untested `nice` idea. Details in the last table.
- **Where a file disagrees with the Lead's summary:** rs-ab refutes "4 removes it"; rs-cost measured 60 named tests, not all ~5,100 (its Verdict); rs-std quotes a 100 ms default period, rs-hist shows 200 ms since 0.9.108 by a scratch run (rs-hist wins: it ran it).

## Evidence

### Cause

The chain: the Gate runs ~5,100 processes (rs-cost Verdict: 4.8-6.6 ms CPU per exec, 68% system time of the sampled tests); at 4-5 Gates the load is 55-100 on 14 cores (rs-hist Verdict 2); nextest's task is late and the timer wins. Hypotheses:

| H | Claim | Status | Deciding finding |
|---|---|---|---|
| H1 | std's `pipe()` + CLOEXEC window lets a sibling test inherit the pipe | exists in std; **killed** for nextest 0.9.148 | rs-src Verdict: nextest has the lock (PR #3553), `nxlock` 0 of 16,000 children inherited. rs-cpd Verdict: 0 of 27,772 real test processes had a stray fd |
| H2 | a test's own child holds the pipe | **killed** for the pure victim and for the synthetic tests (they spawn nothing); open for 4 spawning victims, not needed to explain the data | rs-cpd Verdict (`tree::view` test builds a view); rs-diag Verdict (leaks with tests that spawn nothing) |
| H3a | no holder: nextest's 100 ms timer loses a race under load | **confirmed** (synthetic); rs-src, rs-cpd, rs-os had it open, rs-diag decided it later | rs-diag Verdict: 23 LEAK at 100 ms, 0 at 1 s and 5 s, same pressure; also at `-j 1`: 4 leaks at load 174.5 (rs-diag Evidence); no leak at load 20-24 |
| H3b | something else (sccache, Gate wrapper) | **open**, nothing found | rs-src, rs-cpd, rs-diag Verdicts |
| H3c | the OS delays exec/exit/EOF | **killed** in this form; the narrower "something in the OS stalls nextest's threads" is open | rs-os Verdict: 9,000 spawns, EOF within 0.5 ms of `wait()`; scanners use 10-12% of CPU; APFS, `.noindex`, ramdisk the same. rs-diag: which component stalls nextest was not seen (no sudo) |

Limits (verified in the files): H3a is shown on a synthetic workspace, not on a real Gate with a 1 s period; "no holder seen" is a snapshot, a holder gone before `ps` is not excluded (rs-diag Evidence). Upstream: nextest#1469 is the same symptom, open and undiagnosed (rs-src Verdict).

### The User's five questions

- **Q1, why now (rs-hist Verdict).** Nextest ran on GitHub Actions for 1 day: 92,238 test executions, 0 `LEAK`; one run alone per fresh VM. The Gate on `pro` is macOS with 4-5 Gates at once; the suite grew 3.9 times in 7 days (1,307 to 5,137 tests); test slots capped full runs at 2 until 2026-10-25 18:14 EDT (#425); the repo's 100 ms is half nextest's default since 0.9.108; `result = "fail"` (2026-10-20) made leaks fail runs, it did not create them. Recommendation: a fixed `test-threads` below the cores (pro-d44), keep the check. The fix follows it.
- **Q2, homeserver (rs-hs Verdict).** 182 Gates there since 2026-10-20 with 0 LEAK-FAIL, at load 0.9-3.7 now; exec 2.2-2.4 ms against 6.4-11 ms on `pro` (not a controlled comparison). A Plan runs on one machine and Contexts are machine-local, so moving Gates means moving Contexts: no remote Gate exists. Recommendation: not a fix; one decided one-week experiment is the cheapest test. The fix does not move Gates (follows).
- **Q3, dynamic threads (rs-dyn Verdict).** nextest reads `test-threads` once at the start (an integer, negative, or `num-cpus`; no jobserver, no load rule): dynamic only by a script before the run. Not worth it over the fixed number (inference): it cannot react mid-run, and dividing by Workers (14 / 10 = 1 thread) gives 49 min against `gate_minutes` 30. Recommendation: fixed. The fix follows.
- **Q4, cap = cores with single-threaded tests, or tests taking free Worker slots (rs-dyn Verdict).** C = 14, T = 1 keeps 14 processes (below 42) and should remove the LEAK (inference), but a Gate alone is up to 2,926 s = 49 min against `gate_minutes` 30 (a slot-bound upper bound). Free-slot tests need the engine to count a Gate as T slots: a cross-run coupling pro-d30 abolished; nextest cannot cooperate. Recommendation: neither; cap and T are tied by G x T <= 42. The Lead's 4 holds for a cap of 10, not 14 (worst case 56 processes).
- **Q5, industry standards (rs-std Verdict).** "Partly": nextest is common (6 of 16 projects); 0 of 16 share a host between several full test runs; 0 of 18 set `result = "fail"`; 4 of 16 use retries; omicron caps threads (`--test-threads -2`) for the same reason as this Plan. cpd's deviations: concurrent Gates on one host with default threads (the one the evidence points to, inference), the strict leak result, no retries. Recommendation: a cap is standard, the number 4 is not documented anywhere; keep the strictness. The fix follows.

### What changed

| Item | Value | Source |
|---|---|---|
| Setting | `test-threads = 4`, with a comment (`.config/nextest.toml:22-30`), PR threads #461 | the file; `git log` 34ee8bf3 |
| Why 4 | clean cells had G x T <= 42 processes (3 Gates x 14 threads, and a single Gate), failing runs 56-70; T = 4 gives 10 x 4 = 40 | rs-dyn Verdict |
| Worker cap supported | up to 10 at T = 4 (9 with one urgent Job); 14 would need T = 3 (42) | rs-dyn Evidence 3 table |
| Not changed | `leak-timeout` 100 ms `fail`, no retries, no new test group | `.config/nextest.toml:35` |
| Price, one Gate alone | up to 731 s slot-bound against ~190 s calm at 14 threads: an upper bound (inference); no Gate was run alone at 4 threads | rs-dyn Evidence 4a |
| Price, observed | wall medians of the Summary lines: before 136.5 s (13 Gates, 14 threads), after 104.2 s (9 Gates, 4 threads); the load differed, so this shows no price, it does not show there is none. Proof: 252-271 s with 3 at once. A/B: 59-62 s against 55-61 s per synthetic run | Proof c, a, b |

### Proof

**(a) Lead-run proof** (`/tmp/cpd-leak-flake/proof/summary.txt`, logs `r1-g1.log` ... `r2-g3.log`; every log starts with `Starting 5273 tests across 17 binaries (5 tests skipped)`, verified by `grep`). Main `5d7421ac` (#464), `test-threads = 4`; 3 real Gates at once, two rounds. The file's lines:

```
round 1 start 10-26 09:21:55 load averages: 63.62 23.25 13.33
round 1 end   10-26 09:27:58 load averages: 42.60 43.55 27.76
  r1 g1 rc=0 | Summary [ 265.982s] 5273 tests run: 5273 passed, 5 skipped | LEAK-FAIL lines=0 | FAIL lines=0 | TIMEOUT lines=0
  r1 g2 rc=0 | Summary [ 270.910s] 5273 tests run: 5273 passed, 5 skipped | LEAK-FAIL lines=0 | FAIL lines=0 | TIMEOUT lines=0
  r1 g3 rc=0 | Summary [ 271.030s] 5273 tests run: 5273 passed, 5 skipped | LEAK-FAIL lines=0 | FAIL lines=0 | TIMEOUT lines=0
round 2 start 10-26 09:27:58 load averages: 42.60 43.55 27.76
round 2 end   10-26 09:33:22 load averages: 33.28 42.29 33.04
  r2 g1 rc=0 | Summary [ 251.904s] 5273 tests run: 5273 passed, 5 skipped | LEAK-FAIL lines=0 | FAIL lines=0 | TIMEOUT lines=0
  r2 g2 rc=0 | Summary [ 259.095s] 5273 tests run: 5273 passed, 5 skipped | LEAK-FAIL lines=0 | FAIL lines=0 | TIMEOUT lines=0
  r2 g3 rc=0 | Summary [ 258.638s] 5273 tests run: 5273 passed, 5 skipped | LEAK-FAIL lines=0 | FAIL lines=0 | TIMEOUT lines=0
```

Result: 6 of 6 Gates clean, load 33-64. Limit (inference): 3 Gates x 4 = 12 processes; 3 Gates x 14 = 42 was already clean before the change (rs-dyn Evidence 3, rs-std Verdict), so this proof does not tell 4 threads from 14. The failures came at 4-5 Gates (rs-hist).

**(b) The A/B** (rs-ab Verdict; synthetic workspace of rs-diag, the repo's `nextest.toml` unchanged, 6 runs at once + 4 burners, interleaved, 2026-10-26 10:00-10:07 EDT):

| | runs | LEAK-FAIL per round | total | load at round start | wall per run |
|---|---|---|---|---|---|
| A `-j 14` | 18 | 20, 14, 0 | **34** | 15.8, 42.8, 37.3 | 55-61 s |
| B file's 4 threads | 18 | 4, 3, 1 | **8** | 34.6, 39.1, 49.7 | 59-62 s |

Read: B is about 4 times lower, not zero (rs-ab's Poisson P(X<=8) = 1e-7 is an overstatement: the counts are over-dispersed); P3 was not run. One synthetic run is 1,800 trivial tests; a Gate is longer, so it has more chances to lose (inference, rs-ab).

**(c) Tally of real Gates** (`gatescan.sh`, run by me 2026-10-26 10:09 EDT). Logs with no `Summary` line (did not finish, or another shape) are left out: 10 of 23 before, 4 of 13 after. LEAK-FAIL is counted as tests in the Summary line (the log lists each failing test twice).

| Window | Logs | With Summary | With LEAK-FAIL / FAIL / TIMEOUT | Wall median, p90 |
|---|---|---|---|---|
| before: 2026-10-25 21:00 to 2026-10-26 00:21 (14 threads; the 5,1xx-test suite) | 23 | 13 | 4 (5, 3, 3, 1 tests leaky; 12 tests); 0 FAIL, 0 TIMEOUT | 136.5 s, 342.7 s |
| after: 2026-10-26 00:21 to 10:09 | 13 | 9 | 1 (`policy-scopes/open-tiers-a1`, 01:09, 3 tests); 0 FAIL, 0 TIMEOUT | 104.2 s, 240.3 s |

The before window's last log (`leak-flake/open-threads-a1`, 00:20) is the Gate of the PR that carries the change; whether it ran at 4 threads is unverified. Whether the 01:09 Gate ran at 4 threads is unverified too (the log prints no thread count: rs-cpd Evidence). The load differed: before was night with 4-5 Gates; after has day and night, the research Jobs' own stress runs and a Worker cap of 5 (rs-hist Evidence 1). So 1 of 9 against 4 of 13 is weak evidence by itself (inference): the samples are small and the loads are not the same.

## Reproduction

The tally (`/tmp/cpd-leak-flake/gatescan.sh`, 7 lines, reads `~/.cpd/run/*/logs/*pre-merge*.log`; dates local):

```sh
/tmp/cpd-leak-flake/gatescan.sh '2026-10-25 21:00' '2026-10-26 00:21'   # before
/tmp/cpd-leak-flake/gatescan.sh '2026-10-26 00:21'                      # after; leave out the lines with no Summary
```

The synthetic A/B and the diagnostic: `## Reproduction` of rs-diag (gen.sh, round.sh, burn.sh, batch.sh, table.sh) and of rs-ab (`ab.sh`). The Lead's proof script is `/tmp/cpd-leak-flake/proof.sh`, verbatim below. It fetches `origin/main` of `~/projects/cpd`, makes 3 worktrees, warms them, runs 2 rounds of 3 Gates, writes `proof/summary.txt` and removes the worktrees. Delete its last line (`cpd session send`, the Lead's session id) before running it elsewhere. It runs real Gates: only the Lead may (the adapter refuses them to a Worker).

```sh
#!/bin/bash
# Lead-run proof: 3 real Gates (npm run verify, the pre-merge hook) at once on origin/main, 2 rounds.
set -u
OUT=/tmp/cpd-leak-flake/proof
ME=a6cf2183-0218-445f-a62f-c9f2e576ad8d
S=$OUT/summary.txt
: > "$S"
CLEAN=(env $(env | grep -E '^(CPD_|PI_)' | sed 's/=.*//' | sed 's/^/-u /' | tr '\n' ' '))
cd ~/projects/cpd || exit 2
git fetch -q origin main || exit 2
SHA=$(git rev-parse origin/main)
echo "origin/main $SHA ($(git log -1 --format=%s "$SHA"))" >> "$S"
echo "test-threads on that commit: $(git show "$SHA":.config/nextest.toml | grep -E '^test-threads')" >> "$S"
W=(x ~/projects/cpd.leak-flake.proof-1 ~/projects/cpd.leak-flake.proof-2 ~/projects/cpd.leak-flake.proof-3)
for i in 1 2 3; do git worktree add -q --detach "${W[$i]}" "$SHA" || exit 2; done
for i in 1 2 3; do
  ( cd "${W[$i]}" && "${CLEAN[@]}" timeout 1500 npm ci --no-audit --no-fund > "$OUT/warm$i.log" 2>&1 \
      && "${CLEAN[@]}" timeout 1500 scripts/cargo.sh build --workspace --tests >> "$OUT/warm$i.log" 2>&1; echo $? > "$OUT/warm$i.rc" ) &
done
wait
echo "warm-up rc: $(cat $OUT/warm1.rc $OUT/warm2.rc $OUT/warm3.rc | tr '\n' ' ')" >> "$S"
for r in 1 2; do
  echo "round $r start $(date '+%m-%d %H:%M:%S') $(uptime | sed 's/.*load/load/')" >> "$S"
  for i in 1 2 3; do
    ( cd "${W[$i]}" && "${CLEAN[@]}" timeout 1800 npm run verify > "$OUT/r$r-g$i.log" 2>&1; echo $? > "$OUT/r$r-g$i.rc" ) &
  done
  wait
  echo "round $r end   $(date '+%m-%d %H:%M:%S') $(uptime | sed 's/.*load/load/')" >> "$S"
  for i in 1 2 3; do
    L="$OUT/r$r-g$i.log"
    P=$(sed 's/\x1b\[[0-9;]*m//g' "$L")
    echo "  r$r g$i rc=$(cat $OUT/r$r-g$i.rc) | $(echo "$P" | grep -E '^ +Summary' | tail -1 | sed 's/^ *//') | LEAK-FAIL lines=$(echo "$P" | grep -cE '^ +LEAK-FAIL') | FAIL lines=$(echo "$P" | grep -cE '^ +FAIL ') | TIMEOUT lines=$(echo "$P" | grep -cE '^ +TIMEOUT')" >> "$S"
  done
done
for i in 1 2 3; do git -C ~/projects/cpd worktree remove --force "${W[$i]}" 2>/dev/null; rm -rf "${W[$i]}"; done
git -C ~/projects/cpd worktree prune
echo "cleaned: $(ls -d ~/projects/cpd.leak-flake.proof-* 2>/dev/null | wc -l) worktrees left" >> "$S"
cpd session send "$ME" "Proof run finished. $(cat $S | tr '\n' ';')" --unblocks "the 3-Gate proof run is done; read /tmp/cpd-leak-flake/proof/summary.txt" >/dev/null 2>&1
```

## What this means for the fix

The User's decisions and the open items. The recommendation column is what the findings files and the Plan's brief give; none is decided here.

| Decision | Evidence | Cost | Recommendation |
|---|---|---|---|
| (i) The leak period | 1 s and 5 s: 0 LEAK in 3 rounds each, 100 ms: 23 (rs-diag Verdict); upstream's default is 200 ms since 0.9.108 (rs-hist Verdict, scratch run) | Keep 100 ms: a residual chance of LEAK-FAIL when total load is high (A/B: 8 in 18 runs at 4 threads; `-j 1` at load 174.5: 4; tally after: 1 of 9 Gates). Raise to 200 ms or 1 s: a real leak shorter than the period goes unseen; the real leaks of `ms-leak` lasted 2.4-3.0 s (rs-hs Evidence), so a period of 1 s or less would still have caught them (inference; no leak between 0.1 and 1 s is known either way) | The brief of this Plan forbids raising it: the User weighs it. rs-hist: deleting `period` is a return to the default, not a raise (the Lead decides whether the rule counts it; 200 ms halves the miss rate at most, unverified) |
| (ii) Upstream issue | nextest#1469 is the open, undiagnosed same symptom (rs-src Verdict); rs-diag is a candidate reproduction with no cpd code | A day of the Lead; the outcome is not ours (rs-diag "What this means") | Shortest text from rs-diag Reproduction, to attach: "a workspace of 1,800 trivial tests (gen.sh); 6 copies; 6 runs at once + 4 `yes` burners; `batch.sh $W 3 default 100ms` gives LEAK-FAIL, `batch.sh $W 3 default 1s` (scratch `--config-file`) gives 0". Not filed here |
| (iii) The Worker cap | 10 x 4 = 40 <= 42 (rs-dyn); 14 needs T = 3 (42), or 2 with an urgent Job; Gates at 8 Workers were p90 6, max 8 (rs-dyn Evidence 2); no data above 10 Workers | `gate_minutes` 30 must be checked: 10 Gates at once is a 34.8 min CPU-bound upper bound, span p90 1,847 s at cap 8 (rs-dyn Verdict). rs-ab: 24 processes + 4 burners leaked, so 42 is no guarantee | Cap 10 at T = 4 is what rs-dyn supports; 14 needs T = 3. Measure a real Gate at the new cap rather than infer (rs-dyn) |
| (iv) The homeserver experiment | 182 Gates, 0 LEAK-FAIL, load 0.9-3.7 (rs-hs Verdict); hs is Linux, where nextest takes no spawn lock (rs-hs Evidence) | Contexts are machine-local: Plans move, not Gates; a week of the User's work on hs | Not a fix (rs-hs). The rule written first: any LEAK-FAIL at hs load >= 50 over >= 10 Gates means hs is no answer |
| (v) **Idea, NOT tested:** run the test processes at a lower scheduling priority than nextest, so nextest is scheduled first | None: no result exists | Wrapper scripts exist in nextest 0.9.148 but are experimental: `experimental = ["wrapper-scripts"]` in the config (rs-cpd Evidence used it on 0.9.148; https://nexte.st/docs/configuration/wrapper-scripts/ read 2026-10-26: since 0.9.98, tracking issue #2384). A `run-wrapper` script would `exec nice -n N "$@"` (or `taskpolicy`); the experiment is the A/B of rs-ab with a scratch config outside the repo plus the wrapper (about 15 minutes per rs-ab). rs-hist says the Gate already runs at nice 19 (commit 8b52beca; unverified as current), so only the difference between nextest and its tests would be new | Only an experiment can say; not a recommendation. A wrapper in the repo's config would put an experimental feature on the Gate: the User's call |

Not recommended by the evidence: dynamic threads, slot coupling (pro-d30), a ramdisk or `.noindex` for `target/` (rs-os), retries, a quarantine (the brief).
