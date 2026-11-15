# Cold build time of the Gate's worktree (Job gb-1)

**Status: no valid cold-build measurement exists yet.** Two attempts to measure on this machine failed because the machine, not the build, was the bottleneck. By decision of the Lead (escalation `gb-1-a1-q1`) this Job submits this record and **changes no `Cargo.toml`**; the shared-target-dir measurement is Job gb-2. Nothing below is a before/after number: there is no "after", and no candidate was applied.

Labels: *measured* = read from a command's output in this session; *inferred* = my reading, not proven.

## 1. What went wrong with the measurement (measured)

Command, from the worktree, with a fresh `CARGO_TARGET_DIR` under `$TMPDIR`: `scripts/cargo.sh build --timings --tests --workspace` under `timeout 1500` (`scripts/cargo.sh` starts the shared sccache server and sets `RUSTC_WRAPPER=sccache`).

| Attempt | Start | Load at start (`uptime`) | Result |
|---|---|---|---|
| base1 (foreground) | 01:19:46 | 57.4 / 36.8 / 37.4 | My tool call ended with exit 143 (SIGTERM) after ~20 min. The adapter reported a `follower_exit` SIGTERM. Load had risen to 147.9 / 121.4 / 82.4. The build was at crate 65 or so (`typenum`, `deranged`). Its orphaned `rustc` children still ran under the sccache server (pid 80473). No report was written. |
| base1 (background) | 01:40:39 | 144.6 / 123.9 / 85.1 | Stalled from ~01:45 on the same two crates (`rustc-hash`, `critical-section`): 65 `Compiling` lines at 01:54. Every `sccache rustc` client of my build sat at 0.0 % CPU for 9-11 min. The server's own `rustc` children sat at 0.1-6 % CPU for 5-12 min. Stopped by me at 01:55 (load 83 / 89 / 78). |

Machine state at 01:55 (`top -l 1`, `vm_stat`, `df -h`, `sccache --show-stats`):

- CPU usage 27.6 % user, **72.4 % sys, 0.0 % idle**; load average 83 (peaked at 147.9 at 01:40).
- 1144 processes, 5740 threads, 34 running; PhysMem 46 G used, **~1 GB unused**, compressor holding 7.5 G.
- Data volume 460 Gi, **96 % full**, 21 Gi free.
- sccache: cache **10 GiB of a 10 GiB maximum** (`Cache size` = `Max cache size`); 5050 compile requests, 79.9 % Rust hit rate. *Inferred:* a full cache evicts constantly and a Gate's hit rate drops with it. Not tested.
- The Gates of PRs `clk` and `design` were still running at 01:57 and sat at the same point as my stalled build: `pr-clk.pre-merge.log` (7746 lines) ends at `Compiling errno v0.3.14` (32 `Compiling` lines), `pr-design.pre-merge.log` at `Compiling num_threads v0.1.7` (33). Neither has a `Finished` line yet. At 02:00 the load was 43.7 / 58.9 / 67.1.

The lines 7828-8241 of `pr-clk.pre-merge.log` named in the Job do not exist in the file as read (7746 lines), so the "26 min 44 s" could not be re-checked from it. *Inferred:* the 26 minutes in `Compiling ... Finished` of that incident were a stall of the same kind as mine (clients waiting on a starved sccache server on a thrashing machine), not the cost of compiling 4 crates. The log shows the Gate waiting among the first ~30 third-party crates, all of which sccache should serve from the cache.

## 2. The same Gate on a calm machine (measured, from the logs)

`~/.cpd/run/tests-mock/logs/pr-*.pre-merge.log`, the `Finished` lines of the Gates that finished. Load at the time of these runs was not recorded in the logs.

| Gate log | `Compiling` lines | `Finished `test` profile` in |
|---|---|---|
| pr-audit | 138 | 22.49 s |
| pr-fake-tmux | 138 | 22.14 s |
| pr-off-del / del2 | 138 | 22.74 s / 22.19 s |
| pr-off-pi-check / pr-off-pi | 138 | 22.64 s / 27.32 s |
| pr-tripfix / pr-tripfix2 | 143 | 23.53 s / 22.57 s |
| pr-lint-audit | 143 | 34.62 s |
| pr-off-del3 / del4 | 143 | 36.23 s / 29.32 s |

So a cold Gate build, with sccache, is **22-36 s** when the machine is not starved, 6-10 times less than the "minutes" of the incident and well inside homeserver's 133 s for the whole Gate. Build time is therefore not what makes the Gate slow when the machine is calm; *inferred:* the incident is machine starvation (section 1), which no profile setting fixes.

Unit breakdown readable from `pr-audit` (order of the `Compiling` lines, no timings): 134 third-party crates (`libsqlite3-sys` among them, the bundled SQLite C build), then the 4 workspace crates `cpd-core` (line 7513), `cpd` (7525), `cpd-tui` (7544), `cpd-engine` (7545), all after their dependencies. `--timings` was never obtained, so per-crate seconds, build scripts and link times are **unmeasured**.

## 3. Candidates: where the repo already stands (measured, from the files)

| Candidate | State | Source |
|---|---|---|
| `debug = "line-tables-only"` or `0` | **Won already**: `[profile.dev] debug = 0` (`test` inherits `dev`) | `Cargo.toml` lines 38-39; reason and numbers (full debuginfo 48-62 s and 4.7G, line tables 33-40 s and 2.4G, clean test build) in the comment above; `docs/implementing.md` "Debuginfo" |
| `[profile.dev.package."*"] debug = 0` | **Moot**: `debug = 0` of `dev` already applies to dependencies | the same |
| `split-debuginfo = "unpacked"` | **Moot** with `debug = 0`: there is no debuginfo to split | the same |
| dependencies at `opt-level = 1` | Present: `[profile.dev.package."*"] opt-level = 1`, kept by a measured 35 % cheaper clean build than level 3 | `Cargo.toml` lines 50-51 |
| `incremental = false` for the Gate | **Open.** *Inferred, not measured:* `dev` is incremental by default and sccache does not cache incremental compilations (it counts them under `Non-cacheable calls`: 1226 of 5050 requests at 01:55). The 4 workspace crates are rebuilt in every fresh worktree either way, so the gain is likely small, but only a paired measurement can say | to measure in gb-2 |
| fewer test binaries (merge small areas) | **Open.** Targets from `cargo metadata --no-deps`: 22 test-profile units: libs `cpd-core`, `cpd-engine`, `cpd`, `cpd-tui`; bins `cpd`, `fake-tmux`; 7 `cpd-core` integration tests (`ask_fork`, `distiller_launch`, `hooks_idle`, `migrations`, `repin`, `tier_g`, `worker_tiers`); 7 `cpd` area tests (`engine` 5219 lines, `flow` 16433, `pr` 10040, `session` 5206, `store` 3072, `tooling` 6634, `tui` 12691, each with `cli/tests/common/mod.rs`, 1395 lines, compiled once per binary). Each links against `cpd` and its dependencies. Merging is a test layout change that `docs/research/test-binaries.md` and `docs/implementing.md` ("one test binary per area") decided against; not touched here | to measure in gb-2 |

A backtrace is unchanged by this Job: no setting changed.

## 4. Measurement plan for gb-2 (CPU time as the primary metric)

Load must not matter, so the primary metric is **CPU time**, `user + sys` from `/usr/bin/time -l`, of the whole `cargo build` process tree (rusage of children is included). Wall time and `uptime` before and after are recorded but are secondary.

1. One baseline and one run per candidate, each in a fresh `CARGO_TARGET_DIR=$(mktemp -d)` of the same worktree commit, **the same sccache state** for all of them. Run baseline, candidate, baseline again, to see the drift of the instrument; a candidate counts only when it beats the spread of the two baselines.
2. Take the shared sccache server out of the path, so a stalled server cannot freeze the run: either `SCCACHE_SERVER_PORT=<free port>` with the same `SCCACHE_DIR` for one private server (stop it with `sccache --stop-server` afterwards), or `RUSTC_WRAPPER=` for a run with no cache at all, labelled as such. Check first that the private server answers (`sccache --show-stats`).
3. Command: `/usr/bin/time -l timeout 1500 scripts/cargo.sh build --timings --tests --workspace`, in the background with `nohup`, its output in a file, polled with short commands (a foreground call was killed by the harness at ~20 min).
4. Per run, keep: user+sys seconds, wall seconds, peak RSS (`maximum resident set size`), `uptime` at start and end, the 4 numbers of `sccache --show-stats` (`Compile requests`, `executed`, `Cache hits`, `Non-cacheable calls`) before and after, and `target/cargo-timings/cargo-timing.html`. The unit table of the HTML is the `UNIT_DATA` JSON array: sum duration per unit kind (workspace crates, third-party, build scripts, test binaries).
5. Candidates, in this order (the cheap and likely ones first): `incremental = false` (`CARGO_INCREMENTAL=0`, no edit to try it); `CARGO_PROFILE_DEV_DEBUG=line-tables-only` as the control that reproduces the recorded 48-62 s against 33-40 s; merging two small test areas (a throwaway branch, nothing committed). Keep a candidate only at 10 % or more of CPU time, with a `Cargo.toml` comment that cites the number.
6. Before any run: check `df -h` (the 21 Gi free is room for one 2.4 G target at a time, not for several), `top -l 1` (sys % well below 50) and memory pressure; do not start while the machine thrashes. Separately from the profile: raise or clear the sccache cache that is at its 10 GiB maximum (`SCCACHE_CACHE_SIZE`), after checking it is the cause.

## 5. Shared warm CARGO_TARGET_DIR, sccache only, `.noindex` (Job gb-2)

**Decision: the shared `CARGO_TARGET_DIR` is NOT applied; `scripts/cargo.sh` is unchanged.** It wins by far on time (245 s to 15 s) but it returns wrong results: a worktree whose sources differ from the last build in the shared dir can be served that other build as `Fresh` (section 5.4, reproduced). Make it work comes before make it fast. The `.noindex` name does what it promises (5.5) and is a free win for any target dir that lives outside a hidden directory.

Labels: *measured* = read from a command's output in this session; *inferred* = my reading.

### 5.1 Method (measured)

Machine: 14 cores, load 3-80 from other Workers and Gates during the session (wall times move 2x with the load; **CPU time `user+sys` of `/usr/bin/time -l` does not**, 102-106 s in every cold run). Each "worktree" is a `git archive HEAD` copy under `~/.cache/gb2-measure/wN` (a real worktree would be the same for cargo: files with a fresh mtime; copies, so no real worktree was created or touched), built with `CARGO_TARGET_DIR=<dir> scripts/cargo.sh build --tests --workspace` (the Gate's build, `RUSTC_WRAPPER=sccache` set by the script, one shared sccache server that was already running, its cache at the 10 GiB maximum). Everything was removed afterwards.

### 5.2 Cold build, fresh worktree (measured)

| Run | Target dir | Load at start | Wall | user + sys | Result |
|---|---|---|---|---|---|
| B (sccache only) `base2` | fresh, plain name | 21 | 230.7 s | 87.6 + 16.9 = 104.4 s | the baseline of today |
| B `base3` | fresh, plain name | 8.5 | 245.2 s | 86.7 + 16.8 = 103.4 s | |
| B `spot-plain` | fresh, plain name | 5.5 | 504.2 s | 86.2 + 16.1 = 102.3 s | wall doubled by the load (11 to 23), CPU did not |
| A, first build, fills the shared dir `shared-first` | `shared.noindex`, empty | 21 | 246.8 s | 88.9 + 17.0 = 105.9 s | same as B: the first build of a machine is cold |
| **A, fresh worktree, warm shared dir** `shared-warm2` | `shared.noindex`, warm | 3.2 | **14.6 s** (cargo: 14.10 s) | 58.1 + 16.2 = 74.3 s | **94 % less wall than B (245 s)**, 29 % less CPU |
| A, same again, two worktrees at once (5.3) | `shared.noindex`, warm | 11 | 14.7 s and 14.8 s | 74.2 s and 0.1 s | |

Reuse (measured, `cargo` output): in the warm shared dir only the 4 workspace crates and the test binaries are rebuilt (`Compiling cpd-core`, `cpd-tui`, `cpd-engine`, `cpd`); every third-party crate, build script (bundled SQLite included) and proc macro is `Fresh`. A cold dir recompiles all of them: sccache cannot help there, because it does not cache the crate types `bin` (build scripts) and `proc-macro`, nor incremental compilations (`sccache --show-stats`, non-cacheable reasons at the end of the session: `crate-type` 1239, `incremental` 496, `missing input` 351, `-o` 90). *Inferred:* that is why a cold build costs ~100 CPU-s with sccache at 70-79 % hits (Cache hits barely moved during my cold builds: +2 to +140 of ~140-320 executed per build, 1386 misses in total at the end).

(B) sccache alone **is** the baseline in the first rows: it does not make a cold dir cheap.

### 5.3 Two builds at once into the shared dir (measured)

Two different "worktrees" (`w7`, `w8`, one extra comment line each in `cli/src/main.rs`), started together into the warm shared dir. Both print `Blocking waiting for file lock on package cache`; `w8` also `Blocking waiting for file lock on build directory`. `w7` built (14.51 s); `w8` waited for it and finished at the same second (14.80 s wall, 0.05 s user: it compiled nothing). So **the wait is the whole build of the first**; two Gates at once never build in parallel in one dir. With load 20-80 that serialises Gates machine-wide. The measured case is mild (each build alone is ~15 s warm).

### 5.4 Wrong results (measured, reproduced)

Cargo's `-C metadata` and fingerprint dir names do not contain the worktree path: the shared dir held **11** `cpd-core-*` fingerprint dirs after builds of 10 differently-located trees, the same as a single-tree dir (11). So two worktrees are **the same unit**, not distinct ones (the premise "fingerprints key on the package path" is false). Freshness is then decided by file mtimes (a source newer than the last build's start is dirty), not by content.

Reproduction: a tree `w10` whose `cli/src/main.rs` differs from the shared dir's last build (it prints `GB2-MARKER-W10` at start) and whose files are older than that build (`touch -t 202401010000`), built with the same command: `Finished dev ... in 0.11 s`, the warnings of the earlier build replayed, and `strings target/debug/cpd | grep -c GB2-MARKER-W10` = **0**. The binary the tests of `w10` would run is another worktree's. The same happened, silently, with `w8` above (different content, 0.05 s of CPU).

How it happens in practice (*inferred*): Worker W2 edits `F` at 10:00; Gate or Worker W1 builds into the shared dir at 10:05; W2 builds at 10:10: `F` is older than W1's build, so cargo reuses W1's `F`. Parallel Workers and Gates make this the normal case, and the failure is a green test of code that was never compiled. No timing of the shared dir justifies that.

### 5.5 `.noindex` and Spotlight (measured)

Cold build into a plain name (`~/gb2-spot/target-plain`) against `target.noindex` in the same place, run one after the other, mds/mdworker CPU sampled every 3 s (`ps -axo pcpu,command | awk '/mdworker|mds/{s+=$1} END{print s}'`) during the build and 90 s after it:

- Spotlight index (`mdfind -onlyin <dir> 'kMDItemFSName == "*.d"'`): **190** files of `target-plain`, **0** of `target.noindex`. So macOS skips a `.noindex` dir (previously `unverified`: now measured on this macOS).
- mds CPU sums: plain mean 79.7 %, max 281 %; `.noindex` mean 72.2 %, max 167.5 %. The machine's baseline from other worktrees' `target/` dirs is 72-87 % before either build (`mds-before`), so the difference sits inside the noise of one pair of runs (*inferred*: the two builds' own indexing share is the 8 % mean / 113 % peak difference; not proven).
- A directory under `~/.cache` was not indexed in either name (`mdfind` found 0 `.d` files of `~/.cache/gb2-measure/t-base3` and of `shared.noindex`): *inferred* a hidden parent is skipped too, so the proposed `$HOME/.cache/cpd-target.noindex` is covered either way. `rg -n 'cache|CARGO_TARGET_DIR' scripts docs` finds no existing cpd build cache dir (`~/.cache/cpd-preview` for preview builds only, `docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`).

### 5.6 What stays open

- The 44 plain `target/` dirs of the worktrees are what Spotlight indexes today. A `.noindex` name for *per-worktree* targets would remove that load without sharing: cargo has no `target` name setting, but `CARGO_TARGET_DIR` per worktree (for example `$HOME/.cache/cpd-target.noindex/<worktree-name>`) would give it. *Unmeasured* here, not applied (no win on build time: cold again), and the owner of the Gate environment, not this Job, would have to set it.
- The cost of cold builds is the CPU of build scripts and proc macros that sccache cannot cache; a sound way to reuse them is a per-worktree dir seeded by an APFS clone (`cp -cR`) of a warm dir holding only third-party artefacts. *Untested idea*: the seed must not carry workspace-crate artefacts (the same mtime hazard).
