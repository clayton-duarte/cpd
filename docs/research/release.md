# Release research: semantic versions, CHANGELOG, between-release versions

Question (Context record hs-d299): cpd uses SemVer, releases carry git tags `vMAJOR.MINOR.PATCH` and a CHANGELOG, and the running version shows in `cpd --version` and the right end of the cockpit StatusBar. Today every build says `0.0.0`. This file lays out the options for the User to decide; it decides nothing.

Setup the options must fit (verified on main aade2bb / 070ddee): no GitHub Actions (hs-d283), releases cut on `hs`, squash-merged prose PR titles (`Delete ci.yml: the Gate is the check (hs-d283) (#113)`; 104 of 231 commits carry `(#n)`, the rest predate the squash flow), `publish = false` already in `[workspace.package]`, installs by fast-forward from main on hs, pro and air, and `--locked` release builds (`scripts/install-cpd.sh`), so `Cargo.lock` must be in step with every version bump.

**Evidence labels.** *Source* = read in the cited doc or source. *Ran* = I ran it in a scratch clone of this repo under `/tmp` (removed at the end) on 2026-10-19. *Unverified* = not checked; stated as such. Versions and dates below are those of 2026-10-19 (crates.io `updated_at`, or the GitHub release date).

---

## 1. Release tools

| | release-plz 0.3.172 | cargo-release 1.1.6 | git-cliff 2.14.2 | cocogitto 7.0.0 | knope 0.23.1 | cargo-smart-release 0.21.14 | plain script |
|---|---|---|---|---|---|---|---|
| Runs locally, no CI | Yes: `release-plz update` is local; `release-pr` needs a forge token, `release` can run anywhere (source: rp-usage/update.md, release.md). *Ran* `update`. | Yes; it is a local CLI, dry run by default, `--execute` to act (source: README). *Ran*. | Yes; it only reads git and writes a file. *Ran*. | Yes (`cog bump`, local; README). *Unverified* (not run). | Yes; `knope release` is a local workflow; the README's GitHub Action is an example, not a requirement (source). *Ran* dry run. | Yes; dry run default, `--execute` (source: README). *Unverified* (not run). | Yes. |
| Conventional Commits needed? | No. Non-conventional commits go under `### Other` (source: faq.md). *Ran*: all prose subjects listed, but the prefix `Core:` was parsed as a type and dropped from the line. | No for `cargo release <level>`. Changelog is not its job (source: faq.md "Maintaining Changelog"). | No: `conventional_commits = false` takes any subject; `filter_unconventional = false` keeps unparsed ones (source: config/git.md). *Ran*. | Yes: "not intended to be used in any other context"; a non-goal is supporting other conventions (source: README). Behaviour on prose subjects *unverified*. | No: conventional commits and/or change files; non-conventional commits are ignored (source: Concepts/conventional-commits.mdx). *Ran*: prose history changed nothing. | Conventional messages are "occasional": others land under `Uncategorized` (source: README, `src/changelog/section/segment.rs`). | No. |
| How the bump is decided | Commit types (`feat`, `fix`, `!`) via `update`; override with `release-plz set-version X.Y.Z` (source: update.md, set-version.md). *Ran*: all-prose history gave a patch bump (0.0.0 to 0.0.1). | A flag/argument: `cargo release patch\|minor\|major\|X.Y.Z` (source: reference.md "Bump level"). *Ran*. | `--bump auto\|major\|minor\|patch` or `--bumped-version`; `auto` reads commit types (source: args.md, bump.md). *Ran*: prose history gave a patch bump (v0.1.0 to v0.1.1); no tag gave `0.1.0`. | `cog bump --auto` (commit types) or `--major/--minor/--patch/--version` (source: `crates/cocogitto/src/bin/cog/main.rs`). | Change files `.changeset/*.md` (`default: major\|minor\|patch`) plus conventional commits; the highest wins (source: Concepts/semantic-versioning.md). *Ran*. In 0.x a "minor" change gives a patch bump (source: same page; *Ran*: `minor` file on v0.2.0 gave 0.2.1). | `--bump auto\|major\|minor\|patch\|keep` (source: `src/cli/options.rs`). *Unverified* on prose. | You pass it. |
| crates.io | `publish = true` by default; `publish = false` skips `cargo publish`, but the registry is still used to find the latest release unless `git_only = true`, which reads git tags and skips publishing (source: config.md `publish`, `git_only`). *Ran* with `git_only`. | `cargo publish` by default; a manifest `publish = false` disables it, `--no-publish` too (source: reference.md `publish`). *Ran*: `-vv` printed `publish = false`, nothing was published, no flag needed. | None. | None (`cog` has no publish step). | None; only `Cargo.toml` etc. are edited. | Yes, by design (`crates_index.rs`, "Updating crates-io index"); `--no-publish`, `--no-tag`, `--no-push` exist. `package_may_be_published` is `publish.is_none()` (`src/traverse.rs:368`), so I infer a `publish = false` workspace is skipped. *Unverified*. | None. |
| Edits `Cargo.toml` / `Cargo.lock` | Yes, both; handles `[workspace.package] version` (source: set-version.md). *Ran*: `Cargo.toml` `0.0.0` to `0.0.1`, `Cargo.lock` updated. | Yes, both, workspace-inherited versions too. *Ran*: three crates and `Cargo.lock` moved 0.0.0 to 0.1.0 in one commit. | No. | No itself; `pre_bump_hooks` run commands (source: README "Auto-bumps"). | `Cargo.toml` yes, incl. `workspace.package.version` (source: packages.mdx). `Cargo.lock` only with a `dependency` entry: listing it plainly fails with `cargo_lock_no_dependency` (*Ran*). | Yes (manifests). | You write it (`sed`, or `cargo set-version` from cargo-edit). |
| Creates tag / GitHub release | Tag `v{{ version }}` configurable (`git_tag_name`); GitHub release via `git_release_enable` (source: config.md). `release` *unverified*; it "doesn't push new commits". | Annotated tag, `tag-name = "v{{version}}"`, and push; no GitHub release (source: reference.md). *Ran*: tags `v0.1.0`, `v0.1.1`. | No (changelog only). | Commit plus tag; default tag has no `v`, set `tag_prefix = "v"` (source: docs.cocogitto.io guide/bump, config reference). | `Release` step tags `v{version}` for a single `[package]`; a GitHub release only with a `[github]` config (source: Steps/release.md). *Ran* dry run. | Tags and GitHub release; `--no-tag`, `--no-push` (source: options.rs). | `git tag -a`, `gh release create` if wanted. |
| Maintenance | Release 2026-10-12 (`release-plz-v0.3.172`, GitHub). | 2026-09-16 (GitHub). | 2026-09-18 (GitHub `v2.14.2`). | 2026-03-04 (GitHub `7.0.0`). | 2026-10-19 (GitHub `knope/v0.23.1`). | 2026-09-27 (crates.io `updated_at`). | n/a |

Sources: `https://github.com/release-plz/release-plz/tree/main/website/docs` (`config.md`, `faq.md`, `usage/*.md`); `https://github.com/crate-ci/cargo-release` (`README.md`, `docs/reference.md`, `docs/faq.md`); `https://github.com/orhun/git-cliff/tree/main/website/docs` (`configuration/git.md`, `configuration/bump.md`, `usage/args.md`); `https://github.com/cocogitto/cocogitto` (`README.md`), `https://docs.cocogitto.io/guide/bump` and `/reference/config`; `https://github.com/knope-dev/knope/tree/main/docs/src/content/docs/reference`; the `cargo-smart-release-0.21.14` crate (README, `src/`).

**What this means for cpd**
- **No Conventional Commits needed:** release-plz (weakly), cargo-release (not its job), git-cliff, knope (via change files), a script. **Needs them:** cocogitto (by design), cargo-smart-release (changelog value).
- **Bump decided by a person, not the log:** cargo-release (level argument), knope (change file per PR), a script, or `--bump <level>` on git-cliff / `set-version` on release-plz. Every commit-type bump falls back to *patch* on prose titles (ran on release-plz and git-cliff), which is wrong for a real feature.
- **Tools with crates.io in their core** (release-plz without `git_only`, cargo-smart-release) fight the "never published" setup; the rest are indifferent.
- **Per-crate churn:** release-plz made three `CHANGELOG.md` files (core, cli, tui) by default and a root one needs config (source: `extra/single-changelog.md`). cargo-release ran a hook once per crate (*Ran*: git-cliff wrote into each crate dir until `$WORKSPACE_ROOT` was used).

---

## 2. CHANGELOG

### Source of entries

| Source | How | Quality for cpd | Cost |
|---|---|---|---|
| Squash subjects of main | `git-cliff` (or release-plz) reads `git log` since the last tag | Exactly the PR title + `(#n)`, so as good as hs-d285 titles: one line naming the change. Includes internal lines (`Research: ...`, `Test harness: ...`, `port: ...`). | None per PR. |
| PR titles via `gh` | `gh pr list --state merged --json number,title,labels` (gh 2.45 is installed); git-cliff can fetch PR labels with `--github-repo` and a token (source: git-cliff `integration/github.md`) | Same text as the subject (the subject is the title), so the only gain is **labels** as the grouping key. Needs network, a token and label discipline on every PR. | A label per PR; a token. |
| Change file per PR | knope `.changeset/*.md`, written by `knope document-change` (source: Steps/create-change-file.md) | Best prose, written for readers, and it carries the bump level. | One more file in every PR, a Gate/review rule so it is not forgotten, merge conflicts are rare (one file per PR). |

### Format

| | Keep a Changelog | git-cliff template |
|---|---|---|
| What | Fixed headings `Added / Changed / Deprecated / Removed / Fixed / Security` under `## [x.y.z] - date` (https://keepachangelog.com/en/1.1.0/) | Any markdown you template (Tera); *Ran* a 15-line template producing `## 0.2.0 - date` then `### Changed` and `- subject (sha)` |
| Who writes it | release-plz writes this format by default (source: `changelog/format.md`); knope writes `## x.y.z (date)` with Breaking Changes / Features / Fixes (README) | git-cliff, any group names |

### Grouping prose subjects without Conventional Commits (*Ran*, git-cliff 2.14.2)

```toml
[git]
conventional_commits = false
filter_unconventional = false
tag_pattern = "v[0-9]*"
commit_parsers = [
  { message = "^Release ", skip = true },          # the release commit itself
  { message = "^Research", group = "Research" },
  { message = "^(Delete|Drop|Remove)", group = "Removed" },
  { message = ".*", group = "Changed" },
]
```

Against the 5 newest subjects this produced groups Changed / Removed / Research. Prefix verbs are the only signal prose titles give, so groups stay coarse; a finer split needs labels (above) or change files. The `{ message = ".*", group = ... }` catch-all form is the one in git-cliff's own example (source: `configuration/git.md`, `filter_unconventional`).

### History before the first tag

*Ran*: with no `--tag` range, git-cliff puts **all** history in the first version's section (254 lines for this repo). Options: accept it, or restrict with a range: `git-cliff` takes `[RANGE]` (source: `usage/args.md`), and *Ran* `git-cliff --tag v0.3.0 HEAD~3..` listed only commits after `HEAD~3` (older tags still appear as sections). Alternative: first section says "Initial tagged release. Earlier history: `git log`". The User chooses.

---

## 3. Version of a build between releases

Most running builds are not tagged (install by ff from main every 5 minutes; Worker worktrees; preview binaries). They need a string that says which commit. All options below read git at build time; none changes what `CARGO_PKG_VERSION` says (the release version in `Cargo.toml`).

### 3.1 Does `git fetch origin` bring new tags?

Yes for the form `~/dotfiles/scripts/cpd-install.sh` runs, no for the one `scripts/install-cpd.sh` runs.
- `git fetch origin` (cpd-install.sh, line `git_cpd fetch origin`): git-fetch(1): "By default, any tag that points into the histories being fetched is also fetched". *Ran*: a clone with no tags got `v0.1.0`, then an annotated `v0.2.0`, then `v0.9.0`/`v0.9.1`, each from a bare `git fetch origin`. The remote's configured refspec is only `+refs/heads/*:refs/remotes/origin/*` (same as this repo's), no `tagopt`.
- `git fetch --quiet origin main` (`scripts/install-cpd.sh`): *Ran*: did **not** bring a new tag pointing at main's tip; the next `git fetch origin` did. Not a problem on the 5-minute sync, because cpd-install.sh fetches first. A hand-run `scripts/install-cpd.sh` in a checkout whose last sync predates the tag builds without it.
- Tags must be **pushed** (`git push origin v0.2.0`); a tag on a commit not in any fetched branch is not followed (the same man page). Worktrees share the tags of their repository.
- So pro and air see a tag within one sync (about 5 minutes) of the push, and the build they run after the next fast-forward carries it.

### 3.2 Options

*Ran* = measured by a scratch repo (a copy of this repo's history, plus a throw-away `proto` crate with the `build.rs` of 3.3). "Shape" examples use a real tag `v0.1.0` and a commit 5 later.

| | Plain `git describe` in `build.rs` | `vergen-gitcl` 10.0.3 | `vergen-git2` 10.0.3 | `shadow-rs` 2.0.0 | `git-version` 0.3.9 | `built` 0.8.1 |
|---|---|---|---|---|---|---|
| Mechanism | `build.rs` runs `git describe --tags --match 'v[0-9]*' --always` and prints `cargo:rustc-env` | `build.rs`; shells out to `git describe --always [--tags --dirty]` (source: `vergen-gitcl/src/gitcl/mod.rs`) | Same via libgit2 (`git2` dependency) (source: `vergen-git2` `Cargo.toml`) | `build.rs` writes consts `TAG`, `LAST_TAG`, `COMMITS_SINCE_TAG`, `SHORT_COMMIT`, `GIT_CLEAN`, `PKG_VERSION` ... via `git` commands (source: `shadow-rs/src/git.rs`) | Proc macro `git_version!()`; runs `git describe --always --dirty=-modified` (source: `git-version-macro/src/lib.rs`) | `build.rs`; `GIT_VERSION`, `GIT_DIRTY`, `GIT_COMMIT_HASH_SHORT` via `git2` or `gix` feature (source: `built/src/lib.rs`) |
| Shape, tagged commit | `v0.1.0` (*Ran*) | `v0.1.0` (`VERGEN_GIT_DESCRIBE`) | `v0.1.0` | `TAG=v0.1.0`, `LAST_TAG=v0.1.0`, count 0 (source: `git.rs` docs) | `v0.1.0` if the tag is annotated (default args have no `--tags`) | `v0.1.0` (`GIT_VERSION` "contains HEAD's tag") |
| Shape, between tags | `v0.1.0-5-g070ddee` (*Ran*); `-dirty` if asked | `v0.1.0-5-g070ddee` (doc example `5.0.0-2-gf49246c`) | same | `LAST_TAG=v0.1.0`, `COMMITS_SINCE_TAG=5`, `SHORT_COMMIT=070ddee`: you format it | `v0.1.0-5-g070ddee`, `-modified` suffix if dirty | `0.4.1-10-gca2af4f` per its doc (`git describe`-like) |
| No tags yet | with `--always`: `070ddee` (*Ran*); without it `git describe` exits 128 "No names found" (*Ran*) | `070ddee` (hard-coded `--always`) | `070ddee` (`show_commit_oid_as_fallback(true)`) | `LAST_TAG` empty, `SHORT_COMMIT` set | `070ddee` (`--always` default) | short hash |
| No `.git` (tarball) | `git describe` fails: `fatal: not a git repository` (*Ran*); the script must fall back (*Ran*: `unknown`; it can use `CARGO_PKG_VERSION`) | variable left unset plus a `cargo:warning`, so `env!` fails to compile; set `VERGEN_DEFAULT_ON_ERROR` or use `option_env!` (source: `vergen/src/lib.rs` lines 15, 218) | same | consts empty | macro errors unless `fallback = "..."` or `cargo_prefix` is given (source: macro `lib.rs`) | `None` (`Option<&str>`) |
| Worktree (`.git` is a file) | Works with `git` itself (*Ran*) | `git` works, **rebuild tracking breaks**, see below | libgit2 handles worktrees; same tracking code (`add_rerun_if_changed`) *unverified* | `git` works; no `.git` tracking at all (grep) | `git` works; no tracking | libgit2 works; no `rerun-if` lines (grep) |
| Shallow clone | `--always` gives the sha only when the tag is outside the history (*Ran*, `--depth 1`) | same (`git`) | libgit2: same expected, *unverified*; `built` documents "unable to get a meaningful result" | same as `git` | same | `GIT_VERSION`/`GIT_DIRTY` are `None` on CI platforms (source: `lib.rs`) |
| Rebuild trigger | Whatever the script emits. *Ran* with `rerun-if-changed` on `git rev-parse --git-path` of `HEAD`, `packed-refs`, `refs/tags` and the branch ref: a commit, a tag with no new commit, and a worktree commit all refreshed the string. `-dirty` is **not** refreshed (index and files are not watched). | `build.rs`, `<git-dir>/HEAD`, `<git-dir>/<branch ref>` only (source: `gitcl/mod.rs` `add_rerun_if_changed`). A new tag on the same commit does not rerun. *Ran* a copy of that logic in a worktree: after a commit the string stayed stale (`v0.3.0-1-ge42a908` while HEAD was `7531b69`), because the branch ref lives in the common dir, not the worktree git-dir. This is my replica of the logic, not vergen itself. | same as gitcl (source: `git2/mod.rs` line 635) | `Lazy` default: in debug builds no `rerun-if` lines at all; in release only its own `OUT_DIR` file and env vars (source: `build.rs` `BuildPattern`); no `.git` path appears in the source (grep). I infer it goes stale unless `BuildPattern::Custom` lists `.git` paths. *Unverified*. | None: a proc macro registers no file dependency, so it is re-evaluated only when the crate recompiles for another reason (I infer from the macro source, which reads no file). | No `rerun-if` lines (grep), so Cargo's default applies: rerun when a file in the package changes (Cargo book, build-scripts "change detection"). A commit that touches only `core/` does not touch `cli/`. |
| Build-time cost | No new dependency, one `git describe` process | `vergen`, `vergen-lib`, `bon`, `anyhow`, `time` (build-deps; docs/implementing.md forbids new dependencies without the Lead) | adds `git2` and `libgit2-sys` (C) | `shadow-rs` + `const_format`, `jiff`, `cargo_metadata` ... | proc-macro crates `syn`, `quote` (already common) | `built` (+ `git2` or `gix`) |
| Output a clean SemVer? | You format it | You format it | You format it | You format it | You format it | You format it |

**sccache (hs-d137)** (*Ran*, sccache from `/usr/bin`, release profile, offline): a library crate that reads the version with `env!` and is built through sccache **misses the cache on every new commit, even with identical sources** (hits 0 to 1 on a clean rebuild of the same commit, misses 1 to 2 after an empty commit). The `bin` crate is not cacheable at all (`Non-cacheable reasons: crate-type`), and a debug (incremental) build is not cached either. So: **read the version only in the `cpd` crate (`cli/`), never in `cpd-core` or `cpd-tui`**, or each commit rebuilds a cached library; pass the string to the cockpit as a value at runtime. A release bump of `workspace.package.version` changes `CARGO_PKG_VERSION` for all three crates, so a release rebuilds the three workspace crates once (not the dependencies).

### 3.3 The `build.rs` that was run (about 20 lines, no dependency)

```rust
use std::process::Command;
fn git(args: &[&str]) -> Option<String> {
    let o = Command::new("git").args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}
fn main() {
    // git resolves the paths, so a worktree (.git file) works
    for p in ["HEAD", "packed-refs", "refs/tags"] {
        if let Some(path) = git(&["rev-parse", "--git-path", p]) { println!("cargo:rerun-if-changed={path}"); }
    }
    if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &r]) { println!("cargo:rerun-if-changed={path}"); }
    }
    let v = git(&["describe", "--tags", "--match", "v[0-9]*", "--always"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=CPD_VERSION={v}");
}
```

*Ran* outputs (`0.0.0 / <CPD_VERSION>`): no tags `e79dc0b`; tag on HEAD `v0.1.0`; one commit later `v0.1.0-1-g77e2b09`; worktree after commit and after a tag with no commit refreshed; archive with no `.git` `unknown`. Not wired into cpd (out of scope).

### 3.4 Which string to show

`git describe` gives `v0.1.0-5-g070ddee`. As a SemVer string that sorts correctly after the release, write it as `0.1.0+5.g070ddee` (build metadata): the `-5-g...` form is a SemVer pre-release, which sorts **before** `0.1.0`. Build metadata does not affect precedence, so both versions compare equal to `0.1.0`; this only matters if something compares versions. Decide: show as `describe` prints, or as `0.1.0+5.g070ddee`; on a tag show `0.1.0`.

---

## 4. Ranked recommendation

All three use the same between-release mechanism (the 3.3 `build.rs` in `cli/`, no dependency) and differ in how the release is cut. The between-release string is independent of the release tool; `vergen-gitcl` is a drop-in alternative to the script if the User prefers a dependency (needs the Lead's approval of a new dependency, adds the worktree staleness shown in 3.2 unless the `build.rs` also emits the extra `rerun-if-changed` paths above).

Files in cpd common to all three: `cli/build.rs` (new), `cli/src/cli.rs` (`env!("CARGO_PKG_VERSION")` becomes the build string; the unit test near line 204 and `docs/commands.md` §1.2 change), the TUI StatusBar gets the string from `cli` at runtime, `CHANGELOG.md` (new). **Files in `~/dotfiles`: none** (a plain `git fetch origin` already brings pushed tags, 3.1).

### Option 1 (first): cargo-release + git-cliff

The bump is a word the User types. Fewest moving parts that were all *Ran* end to end.

- **Who decides the bump:** the User, by the level argument; nothing parses titles.
- **Cut a release** (on hs, on an up-to-date main):
  ```
  scripts/cargo.sh install cargo-release git-cliff --locked      # once
  git switch main && git pull --ff-only
  scripts/cargo.sh release minor --workspace --no-push           # dry run: prints bump, hook, commit, tag
  scripts/cargo.sh release minor --workspace --no-push --execute --no-confirm
  git push origin main v0.2.0                                   # tag name = v{version}
  ```
  `release.toml` (new): `allow-branch = ["main"]`, `pre-release-commit-message = "Release {{version}}"`, `tag-name = "v{{version}}"`, `consolidate-commits = true`, and `pre-release-hook = ["sh", "-c", "git-cliff --workdir \"$WORKSPACE_ROOT\" --tag v{{version}} -o \"$WORKSPACE_ROOT/CHANGELOG.md\""]`; `cliff.toml` (new, section 2). `CHANGELOG.md` must already be tracked or the commit omits it (*Ran*: the hook ran once per crate and untracked per-crate files appeared until the `$WORKSPACE_ROOT` form was used).
- **Release commit** edits `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md` (*Ran*). Tag is annotated.
- **`cpd --version`:** release build `0.2.0` (or `v0.2.0`; tag exactly on HEAD); between releases `v0.2.0-5-g070ddee` (or `0.2.0+5.g070ddee`).
- **Costs:** the release commit goes straight to main, or through a PR (see decisions). CHANGELOG grouping is coarse (prefix regexes). `consolidate-commits` and the hook per crate need the settings above.

### Option 2: knope (change file per PR)

The bump and the CHANGELOG text are written per PR, by the author, so they are accurate; the price is a new file in every PR.

- **Who decides the bump:** each PR's change file (`default: patch|minor|major`); the highest since the last tag wins. In 0.x, `minor` bumps patch and `major` bumps minor (source: semantic-versioning.md; *Ran*).
- **Per PR:** `knope document-change` (creates `.changeset/<name>.md`; the Gate could require it, not designed here).
- **Cut a release:**
  ```
  # once: knope.toml with [package] versioned_files = ["Cargo.toml"], changelog = "CHANGELOG.md",
  #   workflow "release": PrepareRelease; Command "cargo update --workspace && git commit -am \"Release $version\""; Release
  git switch main && git pull --ff-only
  knope release --dry-run
  knope release
  git push origin main v0.2.1
  ```
  `Release` tags `v{version}` and, without a forge config, only tags (source: Steps/release.md). `Cargo.lock` cannot be a plain `versioned_files` entry (*Ran*: error `cargo_lock_no_dependency`), so the `Command` step refreshes it; `--locked` builds need it in the release commit.
- **`cpd --version`:** same as Option 1.
- **Costs:** a rule and a file per PR; every PR that changes the product needs one; first release needs the base tag (it reads the latest tag, not `Cargo.toml`: *Ran*, with latest tag v0.2.0 a `minor` file gave 0.2.1).

### Option 3: release-plz, local, `git_only`

Closest to "one tool does everything" but its bump rule is commit types, which prose titles never trigger.

- **Who decides the bump:** `release-plz update` from commit types; on prose titles it picks **patch** (*Ran*). The User overrides with `release-plz set-version X.Y.Z` before committing.
- **Cut a release:**
  ```
  # release-plz.toml: [workspace] git_only = true, git_release_enable = false, git_tag_name = "v{{ version }}"
  #   + a single-changelog section (source: extra/single-changelog.md, extra/single-tag.md)
  git switch main && git pull --ff-only
  release-plz update --config release-plz.toml        # edits Cargo.toml, Cargo.lock, CHANGELOG.md; refuses a dirty tree
  git commit -am "Release 0.1.1"
  release-plz release --config release-plz.toml       # tag only; unverified, not run
  git push origin main v0.1.1
  ```
- **`cpd --version`:** same as Option 1.
- **Costs / *Ran* findings:** per-crate `CHANGELOG.md` in `core/`, `cli/`, `tui/` by default; the `Core:` prefix was eaten as a commit type; it bumped from `Cargo.toml`'s version (0.0.0 to 0.0.1) although a `v0.1.0` tag existed, so `Cargo.toml` must equal the last tag; `cargo-semver-checks` is not installed and is pointless for non-published binaries (it warned and skipped).

### Not recommended

- **cocogitto:** requires Conventional Commits, which hs-d285 rules out; **cargo-smart-release:** built around crates.io publishing and conventional messages; **plain script:** nothing wrong, but Option 1 *is* that script with the bump, `Cargo.lock`, tag and push already handled by an upstream tool (convention over configuration).

### Open decisions for the User

1. **Bump rule:** typed by the User per release (Option 1), by a change file per PR (Option 2), or by commit types with an override (Option 3). Titles stay prose in all three.
2. **Release commit path:** push straight to main (branch protection permitting; I did not check whether main is protected), or open a PR for `Release X.Y.Z`. In the PR path the squash changes the sha, so tag **after** the merge: `git tag -a v0.2.0 <squash-sha> -m "v0.2.0" && git push origin v0.2.0`, and run the tool with `--no-tag`/no push. Unverified end to end.
3. **First version and 0.x:** start at `0.1.0` and stay 0.x (knope and git-cliff then treat breaking as minor), or `1.0.0`. Is `0.0.0` in `Cargo.toml` replaced by the first release commit?
4. **History before the first tag:** summarise (whole history, 254 lines), cut with a range, or one line.
5. **Version string:** `v0.2.0-5-g070ddee`, `0.2.0-5-g070ddee`, or SemVer `0.2.0+5.g070ddee`; show `-dirty` (not refreshed by the `build.rs`, needs extra watching) or omit it.
6. **`build.rs` or `vergen-gitcl`:** no dependency (and 20 lines to own) versus a pinned dependency the Lead must approve.
7. **Which crate shows it:** only `cpd` reads the build string, the TUI gets it as a value (3.2 sccache note), or accept a cache miss per commit in `cpd-tui`.
8. **GitHub release object:** git tag only, or also `gh release create v0.2.0 --notes-file ...` (none of the three does it without a forge token; cargo-release never does).
9. **Where the release tool runs:** hs only (pro and air then see the tag on the next sync).
