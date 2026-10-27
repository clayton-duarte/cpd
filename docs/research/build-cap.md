# Build cap: what `build.jobs` costs and buys

## Conditions

- Machine: Apple M4 Pro, 14 logical CPUs (10 performance, 4 efficiency); shared with other Workers' builds and Gates, never quiet.
- cargo 1.99.1; sccache off (`RUSTC_WRAPPER=` empty).
- Build: `scripts/cargo.sh nextest run --workspace --no-run`, cold, own `CARGO_TARGET_DIR` per build.
- Commit measured: 9d075593.
- Rows labelled `Attempt 1 of cap-m1` were measured there and copied as they are; rows from this Job (`cap-dev-a1`) are marked so.

## Negative jobs

Attempt 1 of cap-m1: 30 independent tiny crates built cold, peak concurrent `rustc` sampled every 0.1 s (about +-1).

| setting | peak concurrent rustc |
|---|---|
| config `jobs = 10` | 11 |
| config `jobs = -4` | 10 |
| env `CARGO_BUILD_JOBS=-4` | 12 |
| config `jobs = -14` (N = CPUs) | 1 |
| config `jobs = -20` (N > CPUs) | 1 |
| config `jobs = 0` | error: `jobs may not be 0` |

Reading: `jobs = -4` behaves like `jobs = 10` (logical CPUs minus 4), in the config file and in the env var; when N is not smaller than the CPU count cargo clamps to 1 job and does not error; `0` is an error. The peaks for 10, -4 (config) and -4 (env) differ by up to 2: I infer sampling noise.

## Dev build matrix

Rows 1-12: Attempt 1 of cap-m1. Rows 13-18: this Job (`cap-dev-a1`), same method; the `foreign procs before` of these rows is the count at the first check, `noisy` means foreign processes were still running after the 5-minute wait, and the label rows read `(cap, n)`. A `noisy` cell had foreign cargo, rustc or nextest processes running; the peak summed CPU column is a saturated 1400 in every row, so it carries no information.

| cap | n | per-build wall (s) | cell total (s) | peak load | peak summed CPU % | load before | load after | foreign procs before | noisy |
|---|---|---|---|---|---|---|---|---|---|
| default | 1 | 112.8 | 112.8 | 70.6 | 1400 | 66.65 | 56.19 | 50 | noisy |
| 10 | 1 | 95.8 | 95.8 | 44.5 | 1400 | 24.42 | 38.92 | 6 | noisy |
| 7 | 1 | 89.5 | 89.5 | 31.2 | 1400 | 22.10 | 26.58 | 2 | noisy |
| 4 | 1 | 98.4 | 98.4 | 31.1 | 1400 | 22.04 | 27.08 | 7 | noisy |
| 2 | 1 | 154.8 | 154.8 | 22.7 | 1400 | 20.93 | 22.35 | 0 | - |
| default | 2 | 109.2,109.2 | 109.2 | 74.2 | 1400 | 37.23 | 52.91 | 4 | noisy |
| 10 | 2 | 135.3,129.6 | 135.3 | 68.0 | 1400 | 31.34 | 44.78 | 4 | noisy |
| 7 | 2 | 98.5,98.5 | 98.5 | 50.5 | 1400 | 19.98 | 39.94 | 0 | - |
| 4 | 2 | 196.5,202.3 | 202.3 | 96.9 | 1400 | 34.00 | 56.16 | 6 | noisy |
| 2 | 2 | 260.4,249.1 | 260.4 | 53.5 | 1400 | 35.51 | 29.06 | 37 | noisy |
| default | 3 | 154.7,148.9,148.9 | 154.7 | 100.7 | 1400 | 32.65 | 64.98 | 7 | noisy |
| 10 | 3 | 167.5,173.5,173.5 | 173.5 | 81.1 | 1400 | 36.30 | 48.84 | 14 | noisy |
| (7, 3) | 3 | 182.8,183.5,185.3 | 185.3 | 82.2 | 1001 | 35.62 | 46.32 | 8 | noisy |
| (4, 3) | 3 | 147.5,148.5,145.8 | 148.5 | 66.0 | 1072 | 53.81 | 49.47 | 19 | - |
| (2, 3) | 3 | 254.9,277.8,267.6 | 277.8 | 62.4 | 1065 | 38.26 | 45.74 | 6 | noisy |
| repeat (default, 1) | 1 | 112.8 | 112.8 | 65.6 | 1072 | 48.17 | 48.65 | 2 | noisy |
| repeat (default, 3) | 3 | 151.1,151.9,153.1 | 153.1 | 93.5 | 1132 | 23.89 | 50.53 | 5 | - |
| repeat (7, 3) | 3 | 183.5,182.2,184.9 | 184.9 | 102.6 | 1072 | 45.26 | 71.01 | 2 | noisy |

## Reading

- Lone build (n=1) against `default` 112.8 s (measured twice, identical): cap 10 -15%, 7 -21%, 4 -13%, 2 +37%; I infer caps 10, 7 and 4 are not distinguishable from `default` or from each other (cells were noisy), only 2 is clearly slower.
- n=2 total against `default` 109.2 s: 7 is 98.5 s, 10 is 135.3 s, 4 is 202.3 s (noisy, load 34-97), 2 is 260.4 s.
- n=3 total against `default` 154.7 and 153.1 s: 10 is 173.5 s, 7 is 185.3 and 184.9 s (+20%), 4 is 148.5 s (-3%), 2 is 277.8 s (+80%).
- n=3 peak load against `default` 100.7 and 93.5: 10 is 81.1, 7 is 82.2 and 102.6, 4 is 66.0, 2 is 62.4.
- Spread of repeats: wall 154.7/153.1 s (1%), 112.8/112.8 s, 185.3/184.9 s (0.2%), so walls repeat tightly; peak load 82.2/102.6 for cap 7 differs by 25%, so peak-load differences under about 25% mean nothing, and cap 7 at n=3 is not distinguishable from `default` in peak load.
- Cap 4 is the only cap that costs a lone build little (98.4 s, no slower than `default`) and clearly lowers peak load at n=3 (66.0 against 93.5-100.7, -29% to -34%); I infer its n=2 result (202.3 s, load before 34, 6 foreign processes) is foreign load, because its n=3 cell (not noisy) had 148.5 s. Cap 2 is clearly too slow.
- `CAP = 4`; for 14 CPUs the `-N` form is `-10` (I infer from `-4` giving 10 jobs; `-10` itself was not measured). Reason: a lone build is not slower than `default` and three concurrent builds peak at 66 load instead of about 97.

### Release

- Lone build, cap 4 against `default` 103.5 s: 68.6 s (-34%, both cells noisy, load before 25.8 and 42.8; I infer the gain is foreign load, not the cap). Cap 2: 117.4 s (+13%).
- n=3 total against `default` 105.9 s: cap 4 is 134.6 s (+27%), cap 2 is 107.8 s (+2%).
- n=3 peak load against `default` 106.4: cap 4 is 55.4 (-48%), cap 2 is 33.3 (-69%).
- Cap 4 buys about half the peak load at n=3 and costs about 27% in wall there; a lone build is not slower. Cap 4 also suits the release build.
- Cap 2 was not slower at n=3 (107.8 s) though its lone build was 117.4 s; load before differed (11.8 lone, 33.3 at n=3, `default` 18.2-42.8), so I infer foreign load swamps small differences and the release graph is mostly one long rustc; not a conclusion.

## Release build matrix

Attempt `cap-rel-a1`. Build: `CPD_COMMIT=measure scripts/cargo.sh build --release --locked -p cpd`, cold, own `CARGO_TARGET_DIR`, `RUSTC_WRAPPER=` empty, `timeout 900`. Caps: `default`, 4 (CAP from cap-dev) and 2 (the next lower cap).

Foreign procs before is the count after the wait (up to 5 minutes); `noisy` means foreign cargo, rustc or nextest processes still ran. Peak summed CPU is `ps` %CPU of all processes, sampled every 1 s.

| cap | n | per-build wall (s) | cell total (s) | peak load | peak summed CPU % | load before | load after | foreign procs before | noisy |
|---|---|---|---|---|---|---|---|---|---|
| default | 1 | 103.5 | 103.5 | 55.10 | 1164.8 | 42.83 | 49.64 | 8 | noisy |
| 4 | 1 | 68.6 | 68.6 | 31.25 | 789.5 | 25.78 | 31.05 | 8 | noisy |
| 2 | 1 | 117.4 | 117.4 | 21.23 | 962.3 | 11.79 | 16.90 | 0 | - |
| default | 3 | 105.9,105.8,105.9 | 105.9 | 106.37 | 1169 | 18.19 | 100.66 | 4 | noisy |
| 4 | 3 | 133.5,133.4,134.6 | 134.6 | 55.44 | 1322.7 | 26.40 | 55.44 | 2 | noisy |
| 2 | 3 | 107.6,107.6,107.8 | 107.8 | 33.29 | 912 | 33.29 | 14.37 | 0 | - |

## One rustc's threads

Sampled every 1 s during the cells: the highest `ps` %CPU of any single `rustc` whose command line carries the build's target dir, and the number of such `rustc` processes. The n=1 cells are the asked ones; the n=3 cells are the same sampling over three builds (rustc count is then summed over the three builds, the %CPU is still one process). %CPU is macOS `ps`, a decayed average, so I infer it lags the instant value.

| cap | n | peak single-rustc %CPU | peak rustc count |
|---|---|---|---|
| default | 1 | 440.5 | 15 |
| 4 | 1 | 386.9 | 4 |
| 2 | 1 | 198.1 | 2 |
| 2 | 3 | 199.3 | 6 |

- At `default` one rustc peaked at 440.5 % CPU (not the 790 % seen before) with 15 rustc processes at once. At cap 4 it peaked at 386.9 % with 4 rustc, at cap 2 at 198.1 % with 2.
- Single-rustc peak falls with the cap (440.5, 386.9, 198.1 for default, 4, 2): the cap 2 peak is near 2 cores and the cap 4 peak below 4. So here `CARGO_BUILD_JOBS` bounded one rustc's CPU too, not only the number of rustc processes. I infer, unverified, that its codegen threads take cargo's jobserver tokens (the cap-2 number fits that; the 790 % case was not reproduced, so cap 4 against it was not measured).

## Gate matrix

Attempt `cap-gate-a1`. The Worker of this Job could not run `npm run verify` (the adapter refuses it; a Worker runs only the tests its change touches). The Lead ran the Gate matrix on commit 976c60b4, one run per cell, on the Apple M4 Pro (14 logical CPUs). Per cell: `timeout 1500 npm run verify` in the Lead's worktree, `RUSTC_WRAPPER=` (sccache off), `CARGO_BUILD_JOBS` unset for `default` and `4` for the cap, one Gate run per cell. `npm ci` was done before the clock started. Before each cell the script waited up to 5 minutes for foreign cargo, rustc and nextest processes: none ran in any cell. `build part` is the time until nextest printed `Starting 5535 tests across 17 binaries`. Every Gate ran 5535 tests, 5535 passed, 5 skipped. The cells ran in the order default/1, 4/1, default/3, 4/3. The script and logs are kept by the Lead outside the repo. n=2 was not run.

| cap | Gates at once | exit | wall per Gate (s) | build part (s) | LEAK-FAIL | cell wall (s) | peak load (1 min) | load before | foreign procs | noisy |
|---|---|---|---|---|---|---|---|---|---|---|
| default | 1 | 0 | 266.9 | 100.8 | 0 | 266.9 | 34.3 | 7.85 | 0 | - |
| 4 | 1 | 0 | 281.6 | 130.8 | 0 | 281.6 | 17.6 | 11.14 | 0 | - |
| default | 3 | 0, 0, 0 | 597.2, 587.2, 597.2 | 207.6, 212.6, 212.6 | 0, 0, 0 | 597.2 | 151.4 | 10.08 | 0 | - |
| 4 | 3 | 0, 0, 0 | 440.9, 430.9, 440.9 | 172.6, 162.6, 172.6 | 0, 0, 0 | 440.9 | 50.5 | 18.01 | 0 | - |

One Gate alone is 5.5 % slower with the cap (281.6 s against 266.9 s) and its build part is 30 s longer (130.8 s against 100.8 s); with 3 Gates at once the cap is 26 % faster per Gate (about 436 s on average against about 594 s: cell wall 440.9 s against 597.2 s), the build part is 20 % shorter (about 169 s against about 211 s) and the peak 1-minute load is a third (50.5 against 151.4); three capped Gates at once (440.9 s) finish before three uncapped ones (597.2 s) and before three capped Gates run one after another (about 3 x 282 = 845 s, computed, not measured). No Gate leaked (LEAK-FAIL 0 in all 8). One run per cell with no repeats: the 1-Gate difference (15 s) is within what noise could be (I infer; the Workers' dev cells repeated within 1 %, but the Gate was not repeated); the 3-Gate difference (157 s, 26 %) and the load difference are large.

## Decision

- `CAP = 4` for the final `.cargo/config.toml`: `jobs = 4`. Its `-N` form for 14 CPUs is `-10` (I infer from `-4` giving 10 jobs; `-10` was not measured).
- Use the number `4`: it is the same on every machine; `-N` scales with the CPU count, and `-N` with N not smaller than the CPU count clamps to 1 job (measured, Negative jobs).
- Evidence is the build matrices only (dev, release), all noisy cells:
  - Lone dev build: 98.4 s at cap 4 against 112.8 s at `default`; not slower.
  - n=3 dev peak load: 66.0 at cap 4 against 93.5-100.7 at `default` (-29% to -34%); total 148.5 s against 153.1-154.7 s. Release n=3: peak load 55.4 against 106.4 (-48%), total 134.6 s against 105.9 s (+27%).
  - One rustc at cap 4 peaked at 386.9 % CPU against 440.5 % at `default` (the 790 % case was not reproduced).
- LEAK-FAIL counts old against new: 0 and 0. Gate wall at n=3 old against new: about 594 s (cell 597.2 s) against about 436 s (cell 440.9 s). Lone Gate 266.9 s against 281.6 s; peak load 151.4 against 50.5.
- Applied: .cargo/config.toml, [build] jobs = 4 (Context decision pro-d116).
- Against capping: the release build at n=3 is 27% slower; walls of noisy cells (load before 20-50 on 14 cores) may hide small differences, so the lone-build gain is not distinguishable from `default`. The lone Gate costs about 5 %, one run per cell. The case for it is lower peak load at n=3.
