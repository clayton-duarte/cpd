# Research: does raising nextest's test-threads shorten CI's Rust test step on hs? (Job threads-ab-note, Plan runner)

An A/B of `test-threads` 4, 6 and 8 for the exact CI command, on hs. Nothing is changed: not `.config/nextest.toml`, not `ci.yml`, not a script or test. Base `cd4bc290` (`origin/main`). Machine hs: 16 CPUs, shared with other Workers' builds and Gates and with the CI runner. Every number says how it was got: **measured** (on hs, 2026-10-28 14:12-14:50 EDT, with the commands in Data), **computed** (arithmetic on measured numbers), **inferred** or **unverified**.

Neighbouring notes are referred to, not restated: `.config/nextest.toml` (the cap of 4, marked PROVISIONAL, and its reason), `docs/research/leak-flake-ab.md` and `leak-flake-verdict.md` (the leak A/B under heavy pressure that set it), `docs/research/build-cycle.md` (where the Gate's time goes).

## Question

The CI review of 2026-10-28 found the Rust tests at about 156 s of a 224 s CI run on hs (median of 15 green runs) with hs 57-73 % idle. `.config/nextest.toml` caps a run at `test-threads = 4`. Its reason was several Gates at once on hs and the Macs (leak check fired); CI now runs one Gate at a time per runner and the Macs run no PR Gate. So: **does a count above 4 shorten CI's `Rust tests` step on hs, with no leak and no failing test?** Zero tolerance for leaks and flakes: a faster step that leaks is worse than the current one.

## Data

### Commands

nextest has the option and its environment variable (measured: `cargo nextest help run`):

    -j, --test-threads <N>   Number of tests to run simultaneously [default: from profile]
                             [env: NEXTEST_TEST_THREADS=]   [alias: --jobs]

So no change of command line is needed; each run is the CI step's command, verbatim, with the variable set (`rust tests` step of `.github/workflows/ci.yml`; `verify:rust` of `package.json`, which keeps the PATH tripwire, the `CPD_COMMIT` export and the `scripts/gate-run.sh` wrapper):

    npm run verify:build                       # once, warms target/ (1 m 33 s, measured)
    NEXTEST_TEST_THREADS=<N> timeout 600 scripts/gate-run.sh npm run verify:rust

Every run in the worktree `/home/clayton/projects/cpd.runner.threads-ab`, started in the background with its output in a file. Per run a shell script outside the worktree took `cat /proc/loadavg` right before it and sampled, every 4 s during it, the processes whose command line contains `nextest` (with each one's working directory), to see what else ran. That the variable took effect is **inferred**, from the wall times falling with the count (the Summary line does not print the thread count); nextest's documented order (command line and environment over the profile) is **unverified** here.

All runs: 6,076 tests run, 5 skipped (measured). Wall time is nextest's own `Summary [..s]` (measured); the build is warm, so it is the test run alone, not `gate-run.sh`'s start-up.

### The nine interleaved runs (4, 6, 8, 4, 6, 8, 4, 6, 8)

Load before = the 1-, 5- and 15-minute loadavg (measured). "CI overlap" = a `cargo-nextest` of the CI runner (`actions-runner/_work/...`) was running (measured from the samples, and from `pgrep -af 'actions-runner.*nextest'`). "Other" = a nextest of another Worker's worktree ran during part of the run (measured; share of the 4 s samples in brackets).

| # | threads | wall (s) | Summary (nextest) | exit | load before | CI overlap | other nextest |
|---|---|---|---|---|---|---|---|
| c1 | 4 | 166.8 | 6076 run: 6076 passed, 5 skipped | 0 | 11.13 9.19 7.07 | **yes** (37 of 41 samples) | |
| c2 | 6 | 131.4 | same | 0 | 7.30 8.61 7.23 | **yes** (15 of 32) | |
| c3 | 8 | 104.1 | same | 0 | 13.45 10.75 8.20 | **yes** (26 of 26) | |
| c4 | 4 | 160.0 | same | 0 | 13.29 11.52 8.75 | **yes** (10 of 40) | |
| c5 | 6 | 107.2 | same | 0 | 5.32 8.94 8.22 | no | none |
| c6 | 8 | 86.7 | same | 0 | 6.17 8.13 8.00 | no | none |
| c7 | 4 | 165.3 | same | 0 | 8.28 8.30 8.07 | no | none |
| c8 | 6 | 122.1 | same | 0 | 7.09 7.39 7.74 | no | yes, one run (15 of 30) |
| c9 | 8 | 88.1 | same | 0 | 8.47 8.27 8.05 | no | yes, one run (6 of 22) |

(One more run at 4 threads, started first with an older sampler and so with no sample record, overlapped a CI nextest, which I saw running during it: 183.5 s, passed, exit 0. It is not in the table or the medians.)

Because CI jobs on hs overlapped c1-c4, each of those was repeated once (the Job's rule), and c8 and c9 too, as the other Worker's run may have mattered. The repeats, in the order 4, 6, 8, 4, 6, 8:

| # | threads | wall (s) | Summary (nextest) | exit | load before | CI overlap | other nextest |
|---|---|---|---|---|---|---|---|
| d1 | 4 | 171.4 | 6076 run: 6076 passed, 5 skipped | 0 | 4.76 7.45 7.81 | no | yes, 2 other runs (42 samples with a process; the run is 171 s) |
| d2 | 6 | 121.4 | same | 0 | 8.12 7.52 7.75 | no | yes, 2 other runs (30 samples) |
| d3 | 8 | 91.6 | same | 0 | 9.47 8.43 8.07 | no | one run, briefly (3 of 23) |
| d4 | 4 | 181.0 | same | 0 | 10.01 8.94 8.29 | no | yes, 2 other runs (44 samples) |
| d5 | 6 | 116.7 | same | 0 | 10.32 9.60 8.65 | no | yes, one run (16 of 29) |
| d6 | 8 | 92.1 | same | 0 | 8.11 9.09 8.58 | no | yes, one run (8 of 23) |

(The counts of "samples" are of processes whose command line contained `nextest`, so one other run can give two lines per sample; read them as "present for much of the run", not as a number of runs. **Unverified** how heavy each of those runs was.)

No run overlapped a CI job's nextest except c1-c4 and the first one; but hs was never alone: other Workers' `cargo nextest` runs in their own worktrees ran in many of them, as in real CI use (inferred: that is hs's normal state).

### Medians and ranges per count (computed from the table)

"No CI" = the runs without a CI-runner overlap (c5-c9 and d1-d6); "all" = every run in both tables.

| threads | runs (all) | median, all (s) | range, all (s) | runs (no CI) | median, no CI (s) | range, no CI (s) |
|---|---|---|---|---|---|---|
| 4 | 5 (c1, c4, c7, d1, d4) | 166.8 | 160.0-181.0 | 3 (c7, d1, d4) | 171.4 | 165.3-181.0 |
| 6 | 5 (c2, c5, c8, d2, d5) | 121.4 | 107.2-131.4 | 4 (c5, c8, d2, d5) | 119.1 | 107.2-122.1 |
| 8 | 5 (c3, c6, c9, d3, d6) | 91.6 | 86.7-104.1 | 4 (c6, c9, d3, d6) | 89.9 | 86.7-92.1 |

- Computed, no CI: **6 threads is 30 % shorter than 4** (119.1 against 171.4 s, a saving of 52 s) and **8 threads 48 % shorter** (89.9 against 171.4 s, 82 s). The ranges of the three counts do not overlap (4: 165-181 s, 6: 107-122 s, 8: 87-92 s). The same holds with all runs (4: 160-181, 6: 107-131, 8: 87-104; the top of 8 and bottom of 6 are 104 and 107 s, c3 under CI overlap).
- Order of magnitude against CI's own figure: the 4-thread runs here (160-183 s) are in line with the review's 156 s median of CI's Rust step (**unverified** that they are the same population: the review's runs are CI jobs, these are worktree runs on the same host).

### Leaks, failures, slow tests

- **Leaks: 0 in 15 of 15 runs** (measured: no `LEAK` line in any output; every exit 0, no `gate-run.sh` / `cpd gate finish` line). **Failing, timed-out or killed tests: 0 in 15 of 15** (no `FAIL`, `TIMEOUT`, `SIGABRT` or `SIGKILL` line). So the rule "if any count shows a leak or a failing test, run that count three more times" was never triggered: the rate is 0 of 5 at each count (15 runs; plus the first 4-thread run, 0 of 1).
- The slowest single test in a run (measured, from the PASS lines): 9.4-13.1 s at every count (4: 9.4-10.1 and 13.1 in d4; 6: 9.8-11.2; 8: 11.1-12.8, 11.4, 9.8, 11.6), far under the 30 s slow limit of the profile and the 90 s kill. No sign that more threads make a test slow enough to matter (5 runs per count: weak).
- Peak number of test processes: **not measured** (it was not cheap to get right without tooling). The leak-flake notes' figure for 4 threads is 42 at once for a clean run of the Gate (read from `.config/nextest.toml`'s comment, not re-measured).

### Load and overlap

- Load before each run: 4.8-13.5 (1 min), 7.4-11.5 (5 min); hs has 16 CPUs (measured), so between 0.3 and 0.8 of a runnable task per CPU. Far below the 33-63 of the A/B that set the cap (`leak-flake-ab.md`).
- hs shared with CI: a CI job's nextest overlapped c1-c4, and this is common, not rare (4 of the first 9 runs). Their wall times are not worse than the rest: c1 166.8 and c4 160.0 s at 4 threads are within the 4-thread range of the others, c2 131.4 s (the highest 6) and c3 104.1 s (the highest 8) are the top of their counts, so the CI overlap may cost 10-15 s at 6 and 8 (inferred from two runs, weak).
- **Nine runs, or fifteen, under a shared host's changing load show a difference only when it is large.** Here the difference between counts (50-80 s) is larger than the spread inside a count (up to 21 s at 4 threads: 160.0-181.0), which is the only reason to read it; a difference of 10 s would not have been visible. A different load (several Gates at once, the case the cap was set for) was **not** tested.
- Nothing was left behind: the processes of the runs and the scripts are gone; the `/dev/shm/gate-run.*` directories of the runs were removed by `cpd gate finish` (measured: none of mine left); the temporary files were in `/tmp` and are removed.

## Options

1. **Keep 4 everywhere.** No change, no risk. CI's Rust step stays about 156 s (review) and 160-181 s here (measured). The data does not say 4 is better than 6 or 8 for any reason except the untested one: several Gates at once.
2. **Raise for CI only, through an environment variable.** `NEXTEST_TEST_THREADS=<N>` in the `env:` of ci.yml's `Rust tests` step (one line of the workflow; the variable beats the file's 4 by inference, see Data). A Worker's local Gate and the Macs keep 4 from `.config/nextest.toml` (the shape the Job names). Count: **8** measured 48 % shorter than 4 (computed), no leak and no failure in 5 runs; **6** measured 30 % shorter, same. This is what the cap's reason supports: that reason is several Gates at once, and CI runs one at a time per runner.
3. **Raise everywhere** (change `test-threads` in `.config/nextest.toml`). It also hits every Worker's local run and the Gates the Workers run in parallel on hs, which is exactly the case of the A/B that set the cap (leaks at 14 threads: 34 in 18 runs, at 4: 8 in 18, `leak-flake-ab.md`), and the Macs. Nothing here measured that case.

## Summary

- **Recommendation: raise for CI only (option 2), to 8.** Supporting number: **8 threads 89.9 s against 4 threads 171.4 s (median of the runs without a CI overlap, computed), 48 % shorter, no leak and no failing test in 15 of 15 runs, 5 at 8 threads** (measured). 6 threads is the cautious step: 119.1 s, 30 % shorter, same 0 in 5. Both ranges are far apart from 4's.
- **How sure the data allows.** Sure that, on one Gate at a time, with load 5-13, hs is faster at 6 and 8 than at 4, because the difference (50-80 s) is larger than the spread (up to 21 s). **Not sure** the step stays free of leaks and flakes: 5 runs per count with 0 leaks bound the rate only loosely (inferred: a leak that happens in 1 of 5 runs would be seen about two times in three; one in 20 would mostly not), and `leak-timeout` is 1 s (`.config/nextest.toml`). Zero tolerance for flakes asks for more runs than nine to fifteen; a week of CI runs at the new count is the stronger test.
- **A CI-only override would need:** one line in `ci.yml`, `env: NEXTEST_TEST_THREADS: 8` on the `Rust tests` step, and a pin test (the repo's way of keeping a workflow line from drifting) that the line is there, with the count. Not done here.
- **What the User must decide:** (a) whether a CI-only override is wanted, and 6 or 8; (b) whether the data is enough (15 runs, 0 leaks) or a longer trial in CI is wanted first; (c) whether a worse case should be tried before: the cap's reason was load 33-63 (several Gates at once), and CI's runner can overlap Workers' Gates on hs (it did here: other Workers' nextest ran during 10 of the 15 runs, with no leak).

## Open questions

- Does the leak rate stay 0 at 8 threads over many runs (the A/B had 18 runs per cell)? Nine to fifteen runs cannot answer.
- The peak number of test processes at 6 and 8 was not measured; the cap's comment ties the 4 to a process budget (42 at once for a clean run, a Worker cap of 10). Does 8 change that budget for the Workers on hs when CI runs at the same time?
- Does CI's runner on the Macs (if one is ever added) need its own count? Not measured: Macs were out of scope.
- Does the step's wall time at 8 shrink further at 10 or 12 (16 CPUs), or does a long single test cap it (the slowest test is 9-13 s; the engine-start tests the cap's `priority` list puts first)? Not tried: the Job asked for 4, 6 and 8.
