# Research: adopt Nx in the cpd repo

Plan `nx-proposal`, Job `proposal-docs`, attempt `proposal-docs-a1`. Date: 2026-11-12 (America/Toronto, EST).
Proposal only. This change touches no build file, no CI workflow and no Gate hook (assumption A1).

**Evidence labels.** Each claim carries one of these tags:
- `[measured]` a timing or count taken from CI logs or a local run (by this attempt or by an explore scout, named where it matters).
- `[read]` read from source in the repo or in a published package.
- `[npm]` checked on the npm registry or in a published tarball.
- `[cited]` taken from another research file, not re-checked here.
- `[estimate]` arithmetic on measured numbers, or judgement. Not a measurement.

Repo base: `a9ed84882` (the base of this worktree and of the explore scouts). The local `origin/main` ref reads `7c0fdc2c5`, but I did not fetch, so I did not measure against it.

## Summary

- **Recommendation: do not adopt Nx now.** Its CI gain on the last 20 merged PRs is small: about 1.2 minutes per PR on average (about 16% of the 7m15 median) `[estimate]`. Nx `affected` saves meaningfully only on the 4 PRs that touch no Rust crate, where it skips the Rust steps. On the other 16, Rust tests cannot be narrowed: 15 of the 20 PRs touch `core`, and a `core` change selects every dependent crate `[measured]`.
- **The one Nx step worth running is a CI-only shadow trial (T1, section 8).** It adds no gate, changes no verdict, and deletes cleanly. It exists to measure what this proposal could not: remote-cache hit rates and parallel overlap.
- **Rust: use native zero-plugin `project.json` targets, not `@monodon/rust`.** `@monodon/rust@3.0.0` depends on `@nx/devkit ^22` while Nx is at 24.0.6 `[npm]`. It infers Rust projects but adds no build, test or lint target, and its executors would bypass `scripts/cargo.sh` (the sccache and `CARGO_INCREMENTAL` wrapper) `[read]`.
- **Remote cache: keep it out of the first step.** Two problems block it: `CPD_COMMIT` changes every commit and so defeats cross-commit cache hits on Rust `[read]`, and a self-hosted cache written by PR jobs on `hs` is a trust boundary that needs a decision `[estimate]`.
- **Module boundaries are the weakest case for Nx.** The repo has no ESLint config, so the TypeScript boundary rule has nothing to run in. Nx sees no Rust module edges, and the 17-module cycle inside `cpd-core` `[cited]` is invisible to it.
- **Simple-install (pro-d569) keeps Nx out of the host upgrade path.** pro-d569 has each host build from CI-green main in an isolated directory, and refuses CI release artifacts `[read: simple-install plan.md]`. A cache-served build would replace that build, so Nx stays out of the host path (section 5).

## 0. Method and assumptions

- Accepted assumptions (from the Job Spec): A1 proposal only; A2 Nx 24.0.6 with a self-hosted S3/HTTP remote cache on homeserver, not Nx Cloud; A3 CI baseline and replay figures as in section 1.
- Sources: the explore files `scout-repo.md`, `scout-ci-replay.md`, `scout-timings.md`, `research-nx.md`, `research-alternatives.md` (all in `~/.cpd/run/nx-proposal/explore/`), plus the checks listed per claim.
- This attempt ran no full suite and no benchmark. Every timing here is a CI log value or a scout's local run, not a new measurement.
- Checks made by this attempt: npm versions and dependencies of `nx`, `@monodon/rust@3.0.0`, `@pellegrims/nx-remotecache-s3@8.1.3`, `nx-remotecache-custom@20.0.0`, `turbo@2.11.11`, `@moonrepo/cli@2.6.5` `[npm]`; the `@monodon/rust` source (`src/graph.js`, `executors.json`) `[read]`; `nx@24.0.6` daemon environment handling `[read]`; the last 20 first-parent merges re-counted by path `[measured]`; the last 10 `ci.yml` runs via `gh` `[measured]`.

## 1. Baseline: current costs and CI timings

### 1.1 What CI runs today

- `ci.yml` runs on every `pull_request` and on `workflow_dispatch`, with no `paths:` filter `[read]`. Every PR runs the full Gate: the job `gate` on the self-hosted runner `hs`, `timeout-minutes: 45`, and `cancel-in-progress` per branch `[read]`.
- The Gate steps mirror `pnpm run verify:steps` in this order: dependencies, migrations, portability, typecheck, pi check, format, Rust build, clippy, web, node tests, Rust tests `[read: package.json:28, ci.yml:34-59]`.
- `clean: false` keeps `target/` and `node_modules/` between runs, so CI is always warm `[read]`. No CI cold build has been measured `[cited]`.
- Nothing in `ci.yml` calls `cpd test changed`. Selection by path does not exist in CI today `[read]`.

### 1.2 Measured CI timings

Job `gate` on `hs` `[measured, scout-timings]`:

| Step (3 runs with step detail) | Range |
|---|---|
| Whole job | 6m41s to 10m57s (9 successful runs) |
| Rust tests (`nextest`, 8 threads) | 3m54s to 4m31s, 54% to 60% of the job |
| Rust build | 49s to 77s |
| Clippy | 37s to 45s |
| Web (typecheck, build, test) | 26s to 31s |
| Node tests | 31s to 39s |
| Queue wait (one run) | about 10m33s (run `42980707029`: 18m02s total, 7m29s job) |

`e2e.yml` (non-blocking, not a required check) `[measured, scout-timings]`: 49s to 60s on three successful runs from 2026-11-09. It has had no successful run since; the ten most recent were cancelled. Current duration: unmeasured.

Last 10 `ci.yml` runs (run durations include queue wait) `[measured]`:

| Result | Count | Duration |
|---|---|---|
| success | 6 | 5m23s to 9m06s, median about 7m15s (scout sample, `scout-ci-replay.md`) |
| failure | 4 | 6m47s to 12m48s |

Fresh `gh` pull at 2026-11-12 09:41 EST `[measured]`: five successful runs (5m23s, 6m52s, 6m55s, 9m01s, 9m06s), median 6m55s. Two newer runs had no conclusion yet when pulled. The two samples overlap but are not the same ten runs, so the baseline is stated as the range 5m23s to 9m06s on success, with a median between 6m55s and 7m15s.

**Failures are a cost.** Four of the last ten runs failed `[measured, scout-ci-replay]`. Each `pull_request` push starts a full Gate (`[read: ci.yml triggers]`), so a fix push pays for a whole Gate again `[estimate]`.

### 1.3 Last 20 merged PRs (replay)

Method: `git log --first-parent a9ed84882 -n 20`, then `git diff --name-only <merge>^1 <merge>` per merge. Counts by path `[measured, re-checked by this attempt]`:

| Area | PRs touching it (of 20) |
|---|---|
| Rust (`core/`, `engine/`, `cli/`, `Cargo.*`, `.config/`, `scripts/`) | 16 |
| web (`web/`) | 9 |
| pi (`pi/`) | 6 |
| migrations (`core/src/store/migrations/`) | 2 `[cited, scout-ci-replay.md]`, not re-counted here |
| docs-only PRs (no code area touched) | 0 |
| `core/` touched | 15 |
| `engine/` touched, `core/` not | 1 |
| no Rust crate path (web or pi only) | 4 |

The 15 `core/` PRs matter most: `core` is a dependency of both `engine` and `cli`, so any `core` change selects all three crates for test (section 6). Nx `affected` cannot cut the Rust suite for those PRs.

### 1.4 Local costs

`[measured, scout-timings and scout-ci-replay, local, loaded machine, indicative only]`:
- Cold `cargo check --workspace`: 34 s. Cold `nextest` build: 85 s.
- Incremental `cargo check --workspace` after one edit: 0.5 s to 3 s. A check with warm `target/`: 12.5 s.
- pnpm web typecheck: 0.7 s to 1.2 s; `vite build`: 1 s to 3 s.

### 1.5 Where the baseline cost sits

- The Rust test run is the largest single step: about 4 minutes of a 7 to 8 minute job.
- Build and clippy together are about 1.5 to 2 minutes.
- The web and node steps are about a minute together.
- Queue wait is outside the job and was up to 10.5 minutes in one measured run.

Any saving has to come from one of these four. Nx `affected` acts only on the first and third, and only where the PR touches no Rust crate (section 2).

## 2. Nx affected, computation cache, task orchestration

### 2.1 Nx `affected`

What it would do here:
- `nx affected -t <targets> --base=origin/main --head=HEAD` selects projects whose inputs changed, plus their dependents `[cited, research-nx.md §1.2]`.
- The repo has no `nx.json` and no `project.json` `[read]`. Nx would need one project per Cargo crate (3), one per pnpm package (`web`), and a project for `pi`.
- `pi` is not a pnpm workspace package. `pnpm-workspace.yaml` lists only `web/` `[read]`. The root package `cpd` holds the pi runtime dependencies `[read: package.json:2,30-39]`. So `pi` as a project means either the root package or a new package. This needs a decision (section 4).
- `affected` sees only the edges it is given. For Rust, `@monodon/rust` would infer the crate edges from `Cargo.toml`, and the native route must write them by hand (section 3).
- Cross-language edges are invisible unless declared. Two exist today: `pi/test/protocol.test.ts:11` reads `core/src/events/kind.rs`, and `pi/skills/skill.test.ts:844` reads `core/src/spec/worker.md` `[read]`. `web/src/generated/types.ts` is tracked and is written by the Rust test `core/tests/contract_types.rs` through ts-rs, invoked by the root `generate:types` script `[read]`. An Nx graph without `implicitDependencies` would miss all three.
- Shared inputs must be declared as global. `testsel` already lists the root inputs that force a full run: `Cargo.toml`, `Cargo.lock`, `rust-toolchain*`, any `build.rs`, `.config/`, `core/src/store/migrations/` and `scripts/cargo.sh` `[read: core/src/testsel/mod.rs:469-478]`. An Nx `namedInputs` list would have to repeat that list. One source of truth would be lost.

Effect on the replay `[estimate, from section 1.3]`:
- 4 of 20 PRs touch no Rust crate. Nx could skip the Rust build, clippy and tests for them. Measured Rust-step sum (build + clippy + Rust tests) on the three step-detail runs: 5m31s, 6m15s and 6m00s, so about 6 minutes per such PR `[measured, scout-timings]`.
- 15 of 20 PRs touch `core`, so the Rust selection is the whole workspace, as it is today. Saving: none.
- 1 PR touches `engine` only, which selects `engine` and `cli`. Saving: small.
- Average: 4/20 × about 6 min ≈ 1.2 minutes per PR, about 16% of the 7m15 median.

### 2.2 Computation cache: runner local

- Nx's local cache stores outputs and replays terminal output on a hit. A hit means the command does not run `[cited, research-nx.md §1.2]`.
- Cargo commands have no declared output files to replay in `target/`. The cached value would be the terminal log plus any declared artefact. The Rust build's real output lives in `target/`, which Nx does not manage `[estimate]`.
- Existing local cache: the Gate already keeps `target/` on the runner (`clean: false`), so a local Nx cache adds a second copy for the same work `[read]`.
- Cache hits skip the command, including the wrapper. `scripts/gate-run.sh` wraps each Gate step to give it a private `TMPDIR` and run the `cpd gate finish` leak check `[read]`. A cached step would skip that check `[estimate, from how a cache hit works]`. This conflicts with the no-leak rule unless the cached targets are exempted.

### 2.3 Computation cache: homeserver remote

- Package: `@pellegrims/nx-remotecache-s3@8.1.3`, peer `nx >=20`, depends on the AWS SDK and `nx-remotecache-custom ^20.0.0` `[npm]`. Last modified 2026-04-11 `[npm]`. Its maintenance beyond that date is unverified.
- Target: MinIO on homeserver (the research names env vars `NX_CACHE_S3_ENDPOINT`, `NX_CACHE_S3_BUCKET`, `NX_CACHE_S3_FORCE_PATH_STYLE` `[cited]`; not checked here).
- **Blocker 1, `CPD_COMMIT` defeats the cache.** `verify:build` and `verify:rust` export `CPD_COMMIT=$(git rev-parse --short=7 HEAD)` `[read: package.json:22-23]`. Its value changes on every commit, so the input hash changes on every commit and Rust steps never hit across commits. Excluding it from the hash would replay a binary with the old version string. The fix is a decision about the version string, not a cache setting.
- **Blocker 2, PR code writes the cache.** PR jobs run on `hs`, so a PR can write cache entries. A hit replays success without running the command. The hash covers the declared inputs; if an input is missed from the hash, a PR could replay a result that its code does not match. The inputs for each target must be declared exactly, and the cache write from PR jobs must be scoped (for example read-only from PRs, write only from `main`). That scoping is a decision for the Lead.
- **Policy line.** pro-d569 refuses CI release artifacts `[read: simple-install plan.md]`. A remote cache of build outputs sits close to that. Limit the remote cache to CI verification results, and keep it out of the host upgrade path (section 5).
- **Benefit: unmeasured.** The only case with a measurable benefit is a re-run of an identical SHA (the failed-job re-run). Four of ten recent runs failed `[measured]`. The hit rate on those re-runs is unknown, and T1 measures it.

### 2.4 Task orchestration

- Nx runs targets with `dependsOn` and `--parallel` `[cited]`. The Gate today is 11 steps in one job, in sequence `[read]`.
- Possible overlap: web (26 to 31 s) and node (31 to 39 s) could run alongside the Rust build. Saving if they overlap fully: about 1 minute, computed from the measured step times `[estimate]`.
- Runner capacity: each `hs` runner runs one job at a time, and `hs` has two `[read: ci.yml comment]`. Overlap inside one job does not change the queue.

## 3. Rust support: @monodon/rust v3.0.0 vs native project.json targets

### 3.1 @monodon/rust v3.0.0

- Last modified 2026-06-01 `[npm]`. Depends on `@nx/devkit ^22.0.0`, `@ltd/j-toml`, `chalk ^4`, `npm-run-path`, `semver`, `tslib` `[npm]`. Peer `@napi-rs/cli ^3.0.0-alpha.55` `[npm]`. Nx 24.0.6 is the current stable `[npm]`.
- The package declares no `engines` field and no Nx peer `[npm]`. It pins `@nx/devkit ^22`, so installing it beside `nx@24.0.6` would bring a second Nx major in. How the package manager resolves that is unverified.
- Project inference: `createNodesV2` on `*/**/Cargo.toml`, one project per non-external crate `[read: src/graph.js]`.
- **Targets it adds: one.** `nx-release-publish`, and only for crates that are not `publish = []`. cpd's crates resolve to `publish = []` through the workspace setting, so none is added `[read: src/graph.js:41-49; cargo metadata]`. The source has a `TODO(cammisuli): provide defaults for non-project.json workspaces` `[read]`. So build, test and lint targets must be declared by hand anyway.
- Executors: `build`, `check`, `lint`, `run`, `test`, `wasm-pack`, `napi`, `release-publish` `[read: executors.json]`. The `test` executor builds its command with `buildCommand('test', …)` `[read]`. Whether it runs `cargo test` or `nextest` is unverified. cpd runs `nextest` with `NEXTEST_TEST_THREADS=8` and the `tripwire` on `PATH` `[read: ci.yml, package.json:23]`, so the executor would need to be configured to match.
- Wrapper bypass: the executors call `cargo` directly. cpd's Rust commands go through `scripts/cargo.sh`, which starts sccache when it is on `PATH` and sets `CARGO_INCREMENTAL=0` when sccache wraps `rustc` `[read: scripts/cargo.sh:5-13]`. Routing through the executor would drop that behaviour unless configured.
- Maintenance: moderate. Its own 3.0.0 release is the Nx 22 migration `[cited, research-nx.md §3.3]`.

### 3.2 Native zero-plugin project.json targets

- One `project.json` per crate (`core`, `engine`, `cli`). Each target runs the repo's own command, for example `scripts/cargo.sh nextest run -p cpd-core` for test, and `scripts/cargo.sh clippy -p <crate> --all-targets -- -D warnings` for lint `[estimate: design]`. The commands stay the ones the Gate runs.
- Edges: Nx has no Cargo parser without a plugin. The crate edges must be written as `implicitDependencies`. The real edges, verified by `cargo metadata` in the scout, are `engine → core`, `cli → core`, `cli → engine`, plus dev edges `cpd-engine → cpd-core` (dev), `cpd → cpd` (dev, self) and `cpd-core` self dev `[cited, scout-repo.md §1]`. Dev edges count for test selection, so they must also be written.
- Risk: a missing edge means `affected` silently skips a crate. The check against `cargo metadata` must be a test, not a comment. This is the main design cost of the native route.
- Cost: three small files, plus the `nx` devDependency, which is a change to `package.json`. Those changes belong to a later Job, not this one.

### 3.3 Verdict

Native targets. They keep `scripts/cargo.sh` in the path, have no inferred behaviour to trust, and add no Nx-22 pin. `@monodon/rust` would add a second pin to the Nx major (22 against 24) and a wrapper bypass, for a target set that is three commands.

## 4. Component boundaries and enforced module boundaries

### 4.1 What enforces boundaries today

| Boundary | Enforced by | Status |
|---|---|---|
| Crate edges `core ← engine ← cli` | Cargo: a crate cannot depend on a crate that depends on it | enforced `[read]`; cycle-free by construction |
| Module edges inside `cpd-core` | nothing | a 17-module strongly connected component `[cited, docs/research/workspace-split.md §1.1]`, not re-measured here |
| TypeScript module edges in `web/` | nothing: no ESLint config in the repo | `find` for `eslint.config.*`, `.eslintrc*`, `deny.toml`, `clippy.toml` returned nothing `[read]` |
| pi ↔ core | nothing: pi tests read core source files | `[read]`, see §2.1 |
| web ← core | nothing: generated file checked in | `[read]`; unverified whether the Gate checks it is in sync |

### 4.2 What Nx would add

- Projects and an explicit graph (`nx graph`), with `implicitDependencies` for the file-read edges in §4.1 `[cited]`.
- Tags and `depConstraints`, enforced by an ESLint rule `[cited, research-nx]`. The rule's exact name is unverified: the `eslint-plugin-nx` tarball did not download in this attempt. That rule is ESLint-only, so it applies to TypeScript projects (`web`, root `pi`) and needs an ESLint config the repo lacks. Adopting it adds a linter, which is scope beyond this proposal.
- For Rust: no import-level rule. Nx sees crates, not modules. The 17-module cycle is invisible to it.

### 4.3 Verdict on boundaries

- Crate boundaries are already enforced by Cargo. Nx adds a view, not an enforcement.
- Module boundaries for TypeScript need ESLint first. That is a separate proposal.
- Module boundaries inside `cpd-core` need a Rust-side tool (clippy lints, or an architecture test). Nx is not that tool. The split plan in `docs/research/workspace-split.md` is the place for that work `[cited]`.
- Goal (3) of the User is not served by Nx on the Rust side. Its strongest case is the TypeScript side, which needs ESLint first.

### 4.4 Proposed project list (only if adopted)

- `core`, `engine`, `cli`: native Rust projects (§3.2).
- `web`: pnpm package `cpd-web-client`.
- `pi`: the root package `cpd` holds the pi dependencies, so `pi` is either the root project (with `pi/` inside it) or a new package. **Decision needed from the Lead:** the root is simplest, but the root also holds the repo-wide scripts that touch every project.
- `docs/`: no project; a PR that touches only `docs/` selects nothing, as with testsel today.

## 5. Fit with simple-install (pro-d569)

Decided in the simple-install plan (`~/.cpd/run/simple-install/plan.md`, D2) `[read]`:
- Upgrade builds on each host from the CI-green `origin/main`, in an isolated directory.
- A dry-run migration on a copy of the store, a backup, a swap, a restart, and a rollback on a failed health check.
- CI release artifacts and Mac GitHub Actions runners are refused.

Fit with Nx:
- **No Nx in the upgrade path.** The host build must compile from source in an isolated directory. A remote cache hit would replace that build with a fetched artefact, which contradicts "build on each host" `[estimate]`. Any Nx target used by the upgrade must have its cache disabled.
- **Host toolchain.** simple-install assumes the host has the build toolchain (its A1) `[read: plan.md]`. Whether the host build also needs pnpm is unverified. The `pnpm run typecheck` line in the upgrade spec (`upgrade-impl-a1.md:20`) is a Worker quick check, not a host step `[read]`.
- **Nx daemon.** The Nx client reads `NX_DAEMON` `[read: nx@24.0.6 dist/src/daemon/client/client.js:121]`. A daemon would be a long-lived process on the host next to `cpd serve`, which is supervised by launchd or systemd with restart on failure. The daemon's own supervision is unverified. Set `NX_DAEMON=false` for host builds if Nx is used at all.
- **Memory.** R1 in the plan names an 8 GiB mac-air as the risk `[read: simple-install summary.md]`. Nx's memory use on that host is unmeasured.
- **Working tree.** Nx keeps a local cache directory; its default location is unverified. An isolated build directory keeps any such cache out of the live tree, which is the pro-d569 requirement.

Verdict: Nx stays out of the host upgrade path. It is a CI-side tool only. If the trial (§8) succeeds, the host build keeps running plain `cargo build` in the isolated directory.

## 6. Comparison with cpd testsel and cargo incremental

### 6.1 What cpd already does

- `cpd test changed` (`core/src/testsel/mod.rs`) runs `git diff` against the merge-base with `origin/main`, plus untracked files. It maps each file to its owning crate and takes the reverse-dependency closure of path dependencies. Root inputs run `--workspace`. Output is `-p <crate>` flags or `--test <area>` `[read, scout-repo.md §4]`.
- It is local only. The Gate does not call it `[read: ci.yml]`.
- It covers Rust only. `pi/`, `web/`, `docs/` and `scripts/tests.sh` select nothing `[read, scout-repo.md §4]`.

### 6.2 Honest comparison

| Question | `cpd test changed` (testsel) | Nx `affected` with native targets | Cargo incremental (+ sccache) |
|---|---|---|---|
| Rust test selection for a `core` change | whole workspace (`-p cpd-core -p cpd-engine -p cpd`) | the same, if edges are written right | not a selection tool |
| Rust test selection for an `engine` change | `engine` + `cli` | the same | not a selection tool |
| Web or pi selection | none | yes, if declared (§2.1) | none |
| Used in CI today | no | no (would need T1) | Cargo yes, via `scripts/cargo.sh`; sccache on `hs` unverified |
| Extra dependency | none | `nx` plus the edge file | none |
| Cross-PR cache | none | yes, once Blocker 1 and 2 (§2.3) are settled | none across machines |

Conclusions `[estimate]`:
- For Rust, `affected` adds nothing that testsel does not already compute, once the edges are right. It would duplicate testsel's job in a second place.
- Cargo incremental already gives the local edit loop: 0.5 s to 3 s for an edit, 12.5 s for a check. Nx would not speed that loop.
- The gain Nx can add over testsel is for web and pi, plus the CI-side cache. Testsel does not select web or pi today, and the Gate runs all web and pi steps on every PR.

## 7. Alternatives

Turborepo, moonrepo and the native path are taken from `research-alternatives.md` `[cited]`, except where a version is checked `[npm]`.

### 7.1 Turborepo (turbo 2.11.11 `[npm]`)

- Cargo support is experimental: `futureFlags.experimentalCargoWorkspaces`, which maps `build`, `test`, `check`, `lint` and `format` to cargo commands `[cited]`.
- Caches only final binaries for Rust; library crates default to `cache: false` `[cited]`. For `cpd`, the Rust test step would not be cached at all.
- Remote cache is REST (OpenAPI v8). Official support targets Vercel. Self-hosting needs a community server `[cited]`.
- Verdict: not better than Nx for this repo. Experimental Rust support is a risk on the Gate's main cost. Revisit when the Cargo support leaves experimental.

### 7.2 moonrepo (`@moonrepo/cli` 2.6.5 `[npm]`)

- Native Cargo and pnpm support, `proto` for toolchain pinning, Bazel RE v2 remote cache, which `bazel-remote` serves `[cited]`.
- Gain over Nx for this repo: native Rust and pnpm handling without the `@monodon` gap. Cost: a second build system beside `scripts/cargo.sh`, and a migration of every Gate command.
- Verdict: the most complete fit on paper. Its adoption cost is the largest, and its measured gain here is the same ~1.2 minutes per PR as Nx, because the Rust test step is still the cost. Not recommended now.

### 7.3 cargo + sccache + pnpm --filter (native)

- Already in place in part: `scripts/cargo.sh` starts sccache when it is on `PATH` and sets `CARGO_INCREMENTAL=0` `[read]`. sccache version (0.19.0) `[cited]`, not checked here.
- pnpm `--filter` runs web and pi scripts in topological order without a cache `[cited]`.
- Gain: compiler-unit cache across machines, with no new meta-tool. Cost: none new.
- Verdict: the cheapest step, and partly done already. Whether sccache is on the `hs` runner is unverified (`ci.yml` installs nothing).

### 7.4 No change

- Cost: zero. Benefit: zero. The Gate stays at 6m41s to 10m57s job time `[measured]`.
- The one saving that needs no dependency: a path rule in `ci.yml` that skips the Rust steps for PRs that touch no Rust path. It gives the same ~1.2 minutes per PR average as Nx `affected` on this sample `[estimate]`. It is a `ci.yml` change, out of scope here, and a separate Job would own it.

## 8. Recommendation, ROI and a reversible CI-only trial

### 8.1 Recommendation

**Do not adopt Nx now.** Its gain is a small share of the PRs, and its cost (a pinned package, config for each project, a cache server on `hs`, a boundary tool that needs a linter) is not repaid by that gain.

Run the **T1 shadow trial** only if the Lead wants data on the two things this proposal could not measure: remote-cache hits on re-runs, and parallel overlap. Decide on that data, not on this estimate.

### 8.2 ROI (estimate)

- Saving per PR, averaged over the sample: about 1.2 minutes (16% of the 7m15 median), from skipping Rust steps on 4 of 20 PRs `[estimate]`. A plain path rule in `ci.yml` (§7.4) gives the same figure, so it is not an Nx-specific gain.
- Saving from parallel overlap: up to about 1 minute on PRs where web and node overlap the Rust build `[estimate]`. Not measured.
- Saving from re-run cache hits: unknown. Four of ten recent runs failed `[measured]`, so re-runs happen. Hit rate unmeasured.
- Cost: `nx` dependency pinned to 24.0.6; 3 `project.json` files and `nx.json`; a `ci.yml` shadow job; a MinIO cache on `hs` with credentials; Nx major upgrades on a cadence of about one every few months (22 to 24 in 2026 `[cited]`); the `CPD_COMMIT` decision; the PR-write scope decision. Human time for adoption: not estimated here.
- Net: low at the sample's mix. It rises only if PRs shift toward web or pi, or if the Rust suite is split.

### 8.3 The T1 trial: CI-only, shadow, reversible

Scope (a later Job, not this one):
1. Add `nx` pinned to 24.0.6 as a devDependency, and `nx.json` with `namedInputs` for the root inputs in `testsel` (the list in `core/src/testsel/mod.rs:469-478`, not a copy: generate it or have a test compare the two).
2. Add `project.json` for `core`, `engine`, `cli` with native targets (§3.2), and `implicitDependencies` for the edges in §3.2, with a test that checks them against `cargo metadata`.
3. Add a separate, **non-required** workflow job `nx-shadow` that runs `nx show projects --affected --base=origin/main` (the `--affected` flag exists in `nx@24.0.6` `[read]`; the `--base` flag is unverified) and compares its Rust projects with `cpd test changed --dry-run` (the flag is read from code, not run `[read: testsel/mod.rs]`). It reports; it does not gate. The Gate and the `gate` check are unchanged.
4. Do not enable the remote cache in T1. Local cache only, and only for the web and pnpm targets, so the trial measures nothing that touches the Rust build.

Success criteria, set before the trial starts:
- No PR where `nx affected` selects fewer Rust projects than testsel does. Any such PR fails the trial.
- Over 20 PRs, the `nx-shadow` job shows the agreement rate and the affected web and pi set.
- Parallel-overlap time measured on real runs, not estimated.

Rollback: delete `nx.json`, the `project.json` files, the devDependency and the `nx-shadow` job. Nothing in the Gate depends on them, so rollback is a revert of one commit.

Remote cache: a separate decision, after T1 passes, because it crosses the PR-write trust boundary (§2.3) and touches pro-d569's refusal of CI artifacts.

### 8.4 Open decisions for the Lead

1. Is T1 worth the trial, or is the no-dependency path rule in `ci.yml` (§7.4) the better first step?
2. `pi` as the root project, or a new package (§4.4)?
3. `CPD_COMMIT` in the cache key: excluded (replay with the old version string) or kept (no cross-commit hits)?
4. Who may write the remote cache: `main` only, or PR jobs too?
