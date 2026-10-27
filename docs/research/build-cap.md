# Build cap: what `build.jobs` costs and buys

## Conditions

- Machine: Apple M4 Pro, 14 logical CPUs (10 performance, 4 efficiency); shared with other Workers' builds and Gates, never quiet.
- cargo 1.99.1; sccache off (`RUSTC_WRAPPER=` empty).
- Build: `scripts/cargo.sh nextest run --workspace --no-run`, cold, own `CARGO_TARGET_DIR` per build.
- Commit measured: 9d075593.
- Rows labelled `Attempt 1 of cap-dev0` were measured there and copied as they are; rows from this Job (`cap-dev-a1`) are marked so.

## Negative jobs

Attempt 1 of cap-dev0: 30 independent tiny crates built cold, peak concurrent `rustc` sampled every 0.1 s (about +-1).

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

Rows 1-12: Attempt 1 of cap-dev0. A `noisy` cell had foreign cargo, rustc or nextest processes running; the peak summed CPU column is a saturated 1400 in every row, so it carries no information.

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
| repeat (7, 3) | | not measured yet | | | | | | | |

## Reading

not written yet
