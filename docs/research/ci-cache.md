# CI cache: sccache did not speed up the rust job (hs-n122)

## Question

Warm CI runs took 339 s after sccache (#18, hs-d137) against 307 s before. Why, and keep sccache on CI or not?

## Data

Source: `gh api repos/.../actions/runs/<id>/jobs` step timestamps of the `rust` job, successful `ci.yml` runs only; sccache stats from the log of run 39130172286. The #18 merge is 2026-10-18 01:28Z. Medians (min-max), seconds:

| Window | Runs | Job total | rust-cache | Build step | Test step |
|---|---|---|---|---|---|
| Before sccache, 2026-10-17 22:02Z to 01:23Z (rust-cache with target dir) | 14 | 307 (255-313) | 4 | 14 (12-16) | 257 (206-268) |
| After sccache, 2026-10-18 03:12Z to 05:30Z (`cache-targets: false` + sccache) | 11 | 341 (318-432) | 2 | 35 (19-45) | 273 (250-355) |
| After, first 5 runs only (03:12Z to 03:40Z) | 5 | 339 (318-349) | 2 | 31 (20-43) | 273 (268-276) |

The 339 s and 307 s in hs-n122 are these windows. Runs after 05:30Z are slower again for other reasons (the tmux 3.8 build/install steps, more tests) and are not comparable.

sccache stats (run 39130172286): 219 compile requests, 125 executed, 117 Rust hits, 5 Rust misses (95.9 % hit rate), average cache read 0.185 s, average cache write 0.485 s, 83 non-cacheable calls (`crate-type`: bin/proc-macro/build-script targets). No read, write or timeout errors.

## Reading

- The cache works: the hit rate is 96 %. It just has nothing to win.
- Before #18, `Swatinem/rust-cache` restored `target/` with every dependency compiled, so a warm Build step was 14 s. What rebuilds on every PR is the workspace crates (`cpd-core`, `cpd-tui`, `cpd`), the 5 misses: they are path crates whose source changed, so sccache cannot hit them either.
- #18 set `cache-targets: false`, so `target/` is no longer restored and every dependency is rebuilt as a compiler call served from sccache's GitHub Actions cache: about 117 hits of 0.185 s each plus process and network overhead. That costs about +21 s in Build (14 to 35) and about +16 s in Test (test-profile dependencies fetched the same way): +34 s in total, matching 307 to 341.
- The rest of the job is not compilation: the Test step is mostly test execution (e.g. `e2e_cross` 20 s, `attempt` 9 s, `driver` 8 s, `e2e` 10 s in the log), which no compile cache can shorten.

## Decision

Drop sccache from the CI `rust` job and restore the `target/` cache. The data does not support sccache on CI: a warm run's compile work is already cached by `rust-cache`, and sccache replaced it with a slower path. `.github/workflows/ci.yml`:

- removed `mozilla-actions/sccache-action`, `SCCACHE_GHA_ENABLED` and the `sccache stats` step;
- removed `cache-targets: false` (the `rust-cache` default, `true`, applies);
- added `RUSTC_WRAPPER: ""` so `scripts/cargo.sh` never picks sccache on a runner (an empty value opts out, per its header).

`scripts/cargo.sh` and local sccache use (one compile cache per machine, many worktrees) are unchanged: there sccache does help, and this Job is about CI only.

## Expected effect and what the next PR run must confirm

- Build step back to about 14 s (was 35 s) and Test step about 255 s (was 273 s) on a warm run, so the job total about 35 s lower than the sccache runs, near the 307 s before (plus the tmux steps and test growth since).
- The `rust-cache` step should log `Restored from cache key ... full match: true` on the second run of a branch or on a PR after a `main` run. The first run after this change is cold (the old key has no target dir): expect a Build step of about 55 s and a Test step about 150-220 s, as in the first runs of 2026-10-16.
- No `sccache` lines in the log.

If a warm run does not show Build near 14 s, the target cache is missing: check the `rust-cache` key and the repository cache size (10 GB limit; the leftover `sccache/` entries in the Actions cache age out in 7 days).
