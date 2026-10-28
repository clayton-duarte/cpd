# Research: is a GitHub-built release binary worth it? (Plan runner, Job release-eval-config)

## Question

The User (2026-10-28 12:2x EDT, Context dotfiles, Plan runner) asked, after CI landed on hs's self-hosted runner and began to run for every PR and every push to main: should `scripts/install-cpd.sh` download a binary built by GitHub Actions instead of compiling it on every machine? This note measures what the build costs, lists the options and recommends. **It does not decide; the User does.** No code, workflow or script changed.

Every number says how it was got: **measured** (on hs, 2026-10-28 13:01-13:10 EDT, with the command shown), **computed** (arithmetic on the numbers here), **inferred**, **read** (from a page, with its URL and date) or **unverified**. Neighbouring notes are referred to, not restated: `docs/research/binaries.md` (one binary or several; the install and reload costs of C3), `docs/research/ci-cache.md` (sccache against `rust-cache` on GitHub-hosted CI, hs-n122), `docs/research/build-cycle.md` (the dev/test build cycle on hs; its sccache and link findings), `docs/research/build-cap.md` (its "Release build matrix" has a release build on a Mac).

## Data

### 1. What an install builds

- `scripts/install-cpd.sh` (read at `20d67fe6`): `CPD_COMMIT=$(git rev-parse --short HEAD) scripts/cargo.sh build --release --locked --manifest-path Cargo.toml -p cpd`, then `target/release/cpd install --root <checkout>`, then copies `contrib/holographic/bridge.py`. Its header calls it the bootstrap: it must exist before cpd does.
- dotfiles' `scripts/cpd-install.sh` runs it when the checkout's HEAD differs from the stamp `~/.local/state/dotfiles/cpd-installed-head` (read), on hs (Linux x86_64), pro (work Mac, arm64) and air (personal Mac, arm64) (the machines are the Lead's facts, not checked here).
- **Release profile: none in `Cargo.toml`** (`grep -n release Cargo.toml` finds nothing; only `[profile.dev]` and `[profile.dev.package."*"]` exist; measured). So the release build is cargo's default: opt-level 3, `lto` off, 16 codegen units, no incremental, no strip (cargo's documented defaults; inferred, not printed by cargo). The `[profile.dev.package."*"]` opt-level 1 does not apply to `--release`.

### 2. Timings on hs (measured)

Machine: `nproc` = **16** (measured). cargo 1.99.0, rustc 1.99.0, sccache 0.7.7 (measured, `~/.cargo/bin/cargo --version`). `df -h /tmp`: 279 G free before (measured). Commit `20d67fe6`, version 0.17.76. One run each, with a scratch `CARGO_TARGET_DIR=$(mktemp -d)/target`, removed afterwards; `CPD_COMMIT=20d67fe6`; sccache on its own server and directory (`SCCACHE_DIR` and `SCCACHE_SERVER_PORT=4998`) so hs's shared cache did not take part. hs was not quiet (1-minute load 7.0 before the first run, 12.6-13.9 before the others; `/proc/loadavg`), so read every time as "under load 7-14 on 16 CPUs".

| run | exact command (from the worktree, `CARGO_TARGET_DIR` and `CPD_COMMIT` exported) | wall (s) | notes |
|---|---|---|---|
| cold, no sccache | `RUSTC_WRAPPER= scripts/cargo.sh build --release --locked -p cpd` | **118.8** (cargo: 1m 58s) | empty target dir |
| cold target, sccache cache empty | `scripts/cargo.sh build --release --locked -p cpd` after `rm -rf $CARGO_TARGET_DIR` | **103.7** (cargo: 1m 43s) | the warming build: 125 Rust misses, 0 hits |
| cold target, sccache warm | same command, target removed again | **9.1** (cargo: 8.99 s) | 125 Rust hits; the workspace crates hit too, because the release profile is not incremental, so sccache caches them |
| "incremental": `touch core/src/lib.rs`, same target dir, sccache | `touch core/src/lib.rs`, then the same command | **2.2** (cargo: 2.10 s) | `touch` changes no content: the 4 rebuilt units were sccache hits (129 hits in all) |
| same, no sccache (added) | `touch core/src/lib.rs`, then `RUSTC_WRAPPER= scripts/cargo.sh build --release --locked -p cpd` | **66.2** (`time`) | cargo recompiled `cpd-core`, `cpd-tui`, `cpd-engine`, `cpd` (4 units) |

- **Binary:** `ls -l target/release/cpd` = **23,001,952 bytes** (23 MB; measured; `binaries.md` reported 12.4 MB at an older commit).
- **sccache helps the install only when the source is unchanged.** Warm cache, same sources: 9 s against 104-119 s. After a merge the workspace crates have new content, so they miss; what sccache still serves is the dependencies, and cargo's own `target/` already keeps those between installs on the same checkout. So **the real install of a moved HEAD costs about the "touch, no sccache" row, about 66 s on hs (inferred: a touch recompiles the same 4 units a real edit does; a real edit was not measured)**. A first install on a new machine costs the cold row, about 1m 44s to 1m 59s on hs. (`build-cycle.md` §2 found the same on the dev profile: sccache shortens the dependency phase, not the workspace crates.)
- **The install is more than the build:** `cpd install` and the engine and views reload (`binaries.md` C3) were not timed here.

### 3. How often it runs

Command, from `origin/main` fetched 2026-10-28 13:01 EDT: `TZ=America/New_York git log --first-parent origin/main --since='2026-10-14' --format=%cd --date=format-local:%Y-%m-%d | sort | uniq -c` (measured; days in EDT, today partial):

| day | merges | day | merges | day | merges |
|---|---|---|---|---|---|
| 10-14 | 7 (partial window) | 10-19 | 55 | 10-24 | 49 |
| 10-15 | 38 | 10-20 | 58 | 10-25 | 112 |
| 10-16 | 4 | 10-21 | 40 | 10-26 | 50 |
| 10-17 | 13 | 10-22 | 34 | 10-27 | 39 |
| 10-18 | 56 | 10-23 | 17 | 10-28 | 16 (until 13:01) |

588 first-parent commits in 15 calendar days (computed), a mean of **about 39 a day**, range 4 to 112. (The Lead's list for 10-21 to 10-28 says 22 for 10-21; this command gives 40 for that day and agrees on the other days. Why it differs is unverified, perhaps a different day boundary.)

- **Per machine, upper bound:** one build per merge would be about 39 builds a day, 39 x 66 s = **about 43 minutes of one machine's CPU a day** (computed, on hs's number; the Mac's incremental is unmeasured).
- **Builds coalesce.** The install runs when dotfiles is applied, and builds the checkout's HEAD at that moment, however many merges came since (inferred from the stamp logic: `HEAD != stamp`). So the real count is the number of applies a day, not the merges. How many applies there are a day is **unverified**; the merge rate only bounds it. An apply after many merges costs one build, the same 66 s.

### 4. A Mac's install: unmeasured here

Not measurable from hs and the User's Mac is busy: **the Mac's install time is unmeasured by this Job.** One reference exists: `docs/research/build-cap.md`, "Release build matrix" (Attempt `cap-rel-a1`, an Apple M4 Pro with 14 logical CPUs, so I infer pro; `CPD_COMMIT=measure scripts/cargo.sh build --release --locked -p cpd`, cold, `RUSTC_WRAPPER=` empty) measured **103.5 s, 117.4 s and 105.9 s** for single and 3-at-once builds without a cap, under load 12-43 (that note marks them "noisy"). That is the same order as hs's cold 104-119 s (computed comparison; a different commit and an older tree).

To time it without installing anything, the User runs this in the cpd checkout:

    time env CARGO_TARGET_DIR=$(mktemp -d) scripts/cargo.sh build --release --locked -p cpd

(It is a cold build; the install on a moved HEAD is the cheaper incremental case, roughly the 66 s / 119 s ratio of hs, about 0.55 of cold, inferred.) The Lead adds the numbers the User brings before the review. The air machine has no number at all.

### 5. GitHub's price (read 2026-10-28 13:01 EDT)

Source: `https://docs.github.com/en/billing/managing-billing-for-your-products/managing-billing-for-github-actions/about-billing-for-github-actions`, read 2026-10-28:

- Included compute per month, for standard GitHub-hosted runners in private repositories: GitHub Free **$12**, Pro **$18**, Team **$18**, Enterprise Cloud $300. Which plan the repo's account has is **unverified**.
- Per-minute rates: Linux 2-core x64 $0.006; **macOS 3-core or 4-core (M1 or Intel) $0.062** (the page lists no other macOS size in that table; whether `macos-latest` is arm64 and how many cores it has is unverified here).
- Artifact storage included: Free 500 MB, Pro 1 GB, Team 2 GB; cache 10 GB per repository (read). Whether self-hosted runners cost Actions minutes: the page's compute rule covers GitHub-hosted runners, and I infer self-hosted ones are free (the page's text on them was not read closely: unverified).
- Computed: one macOS build of 2 minutes costs 2 x $0.062 = $0.124; 39 a day = **$4.84 a day, about $145 a month**, against $12-18 included. The 2 minutes is my guess from hs's and the M4's numbers; a 3-4 core hosted runner is probably slower (inferred), so more. A build of 5 minutes: $0.31 each, about $12 a day. Either way the allowance is gone within days unless the workflow skips most merges (Option 3).

## Options

All options keep `scripts/install-cpd.sh`'s local build for the bootstrap (it must run before cpd exists); they differ in who builds the normal install.

**1. Keep building at install (today).** Cost per machine: about 66 s of CPU per apply that finds a moved HEAD on hs (inferred), 104-119 s for a first install (measured); the Macs are near that (build-cap, above) or unmeasured. Nothing to build, publish, secure or keep alive; works offline and when GitHub is down; the binary is built from exactly the checkout (the commit is compiled in, `docs/versioning.md`). Saves nothing, costs nothing new.

**2. CI builds Linux x86_64 only; hs downloads it.** Saves hs's 66 s per apply (inferred), but hs's runner builds the same thing itself, on the same machine, so the CI release build is another 66-119 s of hs's CPU per push to main (measured, cold, and the incremental figure, depending on whether the runner keeps `target/`). The Macs get nothing: a Linux binary does not run on them. I infer it saves little or nothing: it moves hs's build, it does not remove one.

**3. CI also builds macOS arm64 on a GitHub-hosted macOS runner.** Saves the Macs' builds (unmeasured; build-cap suggests about 2 minutes cold). Costs: $0.062 a minute against $12-18 included (read, §5), about $145 a month at 39 builds of 2 minutes (computed), unless the workflow skips most merges (like `ci.yml`'s concurrency group, which keeps one running and one pending run per branch: a burst builds the first and the last) or a spending limit stops it (usage is blocked once the allowance is used when no payment method is on file, per the page). It needs a clean macOS toolchain and the C build of the bundled SQLite (`libsqlite3-sys`, `build-cycle.md` §2) there. `ci-cache.md` found sccache gave nothing on GitHub-hosted CI and `rust-cache` of `target/` did the work; a cache of `target/` per macOS build is a separate cost.

**4. A self-hosted runner on air (personal); not pro.** No Actions minutes (inferred, §5) and builds arm64 natively. **Not on pro, the work machine: corporate monitoring, and a service that runs repository code with the User's rights on it; that is the User's decision**, not mine. pro could download air's arm64 binary (inferred: both are arm64 Macs; their macOS versions may differ, unverified). Cost: another runner to keep up, and a build that exists only when air is on and awake (inferred). The trigger rule below applies even more: a runner on a personal machine must never run PR code.

**5. Cross-compile aarch64-apple-darwin from hs.** Builds the Mac binary on hs, with no new machine. It needs (unverified, none of it tried): the Rust target `aarch64-apple-darwin`; a macOS SDK (Apple's licence limits where it may live; tools such as `osxcross`, or `cargo-zigbuild` with zig as linker and C compiler); a C cross-compiler for the bundled SQLite; and a Mac to run the result on, because the Gate's tests run on hs only, so the binary is shipped untested on its own platform. The most fragile option.

| option | saves per machine | new cost |
|---|---|---|
| 1 today | nothing | nothing |
| 2 Linux only | hs: about 66 s per apply (inferred); pro, air: 0 | a second hs build per push; publishing; fallback |
| 3 GitHub macOS | pro, air: their builds | about $145 a month at 39 builds of 2 minutes, over the $12-18 included (computed) |
| 4 air runner | pro, air: their builds (arm64) | runner on a personal machine; pro's use is the User's decision |
| 5 cross-compile | pro, air: their builds | SDK, linker, untested binary |

## What would change in cpd and dotfiles for the option I would favour, if the User wants one

My favoured shape is **option 3 or 4 plus Linux on hs, published as a per-commit Actions artifact**, with the local build kept as the fallback. I do not favour it over option 1 (see Summary); this is what it would take.

- **Bootstrap.** `install-cpd.sh` keeps its `cargo build`. A new step in front asks for the artifact of the checkout's commit and, if there is none, builds as today. That also covers GitHub being down, `gh` not logged in, a push whose workflow has not finished (a release build takes at least about 2 minutes, measured; an apply right after a merge will miss: inferred) and the burst rule (the concurrency group skips middle commits, so most commits would have no artifact: inferred from `ci.yml`).
- **Finding the artifact for the checkout's commit.** The workflow's run is keyed to the commit's full SHA; the install runs something like `gh run list --workflow <name> --commit <sha>` and `gh run download` (needs `gh` logged in for a private repo on each machine; `gh` is on hs, 2.45.0, measured; the Macs unverified). The compiled-in `CPD_COMMIT` must be the string `git rev-parse --short HEAD` gives in the checkout (its length depends on the repository), so the workflow computes it the same way, on a full-history checkout.
- **Checksum.** The workflow writes `cpd.sha256` beside the binary and the install checks it before `cpd install`. That detects a damaged download, not a tampered one: both come from the same run. (Artifact attestations would do more; whether a private repository on this plan has them is unverified.)
- **Version and tag rules (hs-d584, hs-d585).** The version is `0.<protocol>.<migrations>-<commit>`: the commit is in the binary, so **a binary belongs to a commit, not to a `v<version>` tag**. `v<version>` is placed once per version, on the commit that last changed `Cargo.toml` (`docs/versioning.md`, Tags), not per merge. So the artifact must **not** be a GitHub Release keyed by the tag: that would be one release per merge, or the wrong commit. An Actions artifact (per run, `retention-days` 1 to 2) needs no tag, leaves `scripts/tag-version.sh` (run at the end of the install on any machine) as the only thing that tags, and fits the retirement of release tooling: nothing is released, a build is reused.
- **Trigger.** Only `on: push: branches: [main]` (and `workflow_dispatch`), **never `pull_request`**: the runner on hs runs the code it is given as the User (`ci.yml`; the repo is private and only we push, but a PR's code is not yet a merged commit). A publishing workflow must not share a trigger with the PR Gate.
- **Permissions.** An Actions artifact needs `contents: read` (the workflow's current default) and uploads with the job's token; I infer no `contents: write` is needed (unverified for this repo's settings). A GitHub Release or a tag pushed by the workflow would need `contents: write`, the one place a workflow would push to the repo without going through cpd, against "every push goes through cpd" (the `pr` policy). The artifact route avoids it, another reason to prefer it over a Release.
- **Artifact storage.** 23 MB (measured) x 39 a day with a 1-day retention is up to about 0.9 GB held (computed, ignoring compression and the second platform), against 0.5-2 GB included (read, §5): a limit to check, and a reason to build only the last commit of a burst.
- **GitHub down.** The install falls back to the local build, so nothing blocks; only the time saved is lost.

PRs it would take, one line each, not written:

1. cpd: workflow `release-build.yml` on push to main (and `workflow_dispatch`, never a PR) that builds Linux x86_64 (and macOS arm64) with `--release --locked`, sets `CPD_COMMIT` as the install does, uploads the binary and `cpd.sha256` as a short-retention artifact.
2. cpd: `scripts/fetch-cpd-artifact.sh` (portable sh, inside `check-portable.sh`) that finds the artifact for a commit with `gh`, downloads it, checks the checksum, prints the path or exits non-zero.
3. cpd: `install-cpd.sh` tries the fetch, then falls back to the local build; a test with `gh` mocked (no real network, per the tests policy).
4. cpd: `docs/versioning.md` notes the artifact route and that tags are unchanged.
5. dotfiles: only if `gh` login or the apply timing needs a change (unverified that anything does).
6. dotfiles (option 4 only): a runner service on air; pro is the User's decision.

## Summary

**Recommendation: measure the Mac first, and keep option 1 meanwhile** (one command, §4).

- hs builds the release binary in about **66 s** on a moved HEAD (inferred from a measured `touch` rebuild) and **104-119 s** cold (measured, under load 7-14); the one Mac reference, build-cap's M4 Pro, is **103-117 s** cold (measured there, under load). The saving of any option is about one to two minutes of one machine's CPU per apply, and applies coalesce merges (inferred).
- Option 2 saves hs nothing real. Options 3 to 5 are what would help the Macs, and each costs something real: about $145 a month beyond the included $12-18 for GitHub macOS at 39 builds of 2 minutes (computed), or a runner on a personal Mac plus a corporate-machine question, or an untested cross build. All need a publish-and-fetch path, a checksum, a fallback and tests, and the first apply after a merge usually misses the artifact (inferred), which falls back to the local build.
- **The number that would change the answer:** a Mac install (cold or on a moved HEAD) above about **5 minutes** (my guess, not a measurement), or a Mac applying many times a day, would make option 3 or 4 worth its cost. If the User's `time` is near 2 minutes, do nothing.
- **What the User must decide:** (a) whether 1-2 minutes per apply on a Mac is a pain worth a new moving part; (b) whether a runner on air is acceptable, and whether one on pro is allowed at all; (c) whether GitHub-hosted macOS minutes may be spent past the included allowance (plan unverified); (d) if any of this goes ahead: an Actions artifact keyed by commit, not a GitHub Release, so hs-d584, hs-d585 and "every push goes through cpd" stay as they are.

## Open questions

1. How long does the User's Mac take? Run the command in §4 (cold) and, for an install, time `scripts/install-cpd.sh` once on a moved HEAD. air has no number.
2. How many applies a day does each machine run? Unverified; only the merge rate is known.
3. Which GitHub plan owns the repo (Free, Pro or Team)? It sets the $12-18 compute and the artifact storage in §5.
4. A real edit's release rebuild on hs (not a `touch`) was not measured; I infer the same 66 s.
5. Does a downloaded, unsigned arm64 binary need anything on a Mac (quarantine, ad-hoc signature)? Unverified; a build on the Mac itself does not raise it.
6. Would a macOS build on a GitHub-hosted runner take 2 minutes? Unmeasured; hosted macOS runners may be slower than the Mac in build-cap.
