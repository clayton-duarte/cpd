# The version

cpd's version is `0.<protocol>.<migrations>-<commit>`, for example `0.13.45-a4cedc3`. The three parts are:

- `<protocol>` is an integer, raised when an Event kind, an Event `data` field the pi adapter reads, or the output of a command the adapter runs changes in a way the adapter depends on (contract §5.2 "The protocol number"). Today it is **24** (constant `PROTOCOL` in `core/src/events/kind.rs`).
- `<migrations>` is the count of `.sql` files directly in `core/src/store/migrations/`: the 14 legacy files `0001-initial`..`0014-register` plus the named `<YYYYMMDDTHHMM>-<slug>.sql` files. Today it is **45**. The count only grows (contract §4.5 N5): files are never edited or deleted; a breaking migration may be renamed to a newer timestamp with an unchanged body.
- `<commit>` is the short commit hash the build was given in `CPD_COMMIT`, for example `a4cedc3`. It is capped at 7 characters, matching GitHub's 7-character short SHA: any commit hash in `CPD_COMMIT` longer than 7 characters is truncated to at most 7 characters (its first 7). When `CPD_COMMIT` is unset or empty (a plain `cargo build`), the version is `<semantic version>-dev`, for example `0.13.45-dev`.

cpd will remain at version `0.*` (pre-1.0) until the User decides otherwise.

## Cargo.toml

The workspace version (`[workspace.package] version` in `Cargo.toml` and the four workspace crates in `Cargo.lock`: `cpd`, `cpd-core`, `cpd-engine`, `cpd-tui`) is derived from the PROTOCOL and migration count and kept equal by:

- `scripts/sync-version.sh`: a POSIX sh script that reads `PROTOCOL` from `core/src/events/kind.rs` and counts `core/src/store/migrations/*.sql`, then writes `0.<protocol>.<migrations>` to `Cargo.toml` and the four `Cargo.lock` entries. On success it prints the derived version and exits 0; on failure (no PROTOCOL line in kind.rs or no .sql files in the directory) it prints a `sync-version:` line on stderr and exits 1 without changing any file.
- `--print` flag: `scripts/sync-version.sh --print` prints the derived version to stdout and writes nothing.
- A cpd test (`version::cargo_version_is_zero_protocol_migration_files`) fails with a message naming `scripts/sync-version.sh` when the workspace version differs from the derived version.
- `scripts/new-migration.sh <slug> additive|breaking` runs `scripts/sync-version.sh` after creating a migration file, so Cargo.toml follows the new file.
- After raising `PROTOCOL` by hand in `kind.rs`, run `scripts/sync-version.sh` to update Cargo.toml.
- Two Workflows that each add a migration or raise `PROTOCOL` both rewrite the same line of Cargo.toml. The Gate of the second Workflow's pull request fails until the sync is run again: that is the check working.

## Tags

`scripts/tag-version.sh`, run at the end of `scripts/install-cpd.sh` on any machine, tags the commit of `origin/main` that last changed Cargo.toml's workspace version with the annotated tag `v<version>` (no hash) and pushes it. It:

- Fetches `origin/main`.
- Reads the workspace version of `origin/main:Cargo.toml`.
- Does nothing when `origin` already has the tag `v<version>`.
- Otherwise tags the commit `git log -1 --first-parent -G'^version = ' --format=%H origin/main -- Cargo.toml` (it must hold that version, else the tag-version: command fails) as an annotated tag `v<version>` and pushes it.
- A lost race is not a failure when the remote tag is already on the same commit.
- It acts on `origin/main` whatever the checkout.
- Failures warn on stderr with the prefix `tag-version:` and the next install retries.

## Changelog

No `CHANGELOG` file is kept in the repo. Between two release tags, run `git cliff v<old>..v<new>` on demand with the `git-cliff` and `cliff.toml` pinned in `~/dotfiles` on `hs`. The commit subjects are the pull request titles plus ` (#<number>)`.

## What retired

With cpd's transition to derived versioning:

- **Retired:** `scripts/release.sh` and its test, `release.toml`, `CHANGELOG.md`, `docs/release.md` (replaced by this file), and the `cargo-release` pin in dotfiles.
- **Unchanged:** `PROTOCOL` raises, migration files, `scripts/new-migration.sh` and `scripts/install-cpd.sh` remain; `-<commit>` and `-dev` in the build string (contract hs-d315 (1)–(5), except those two parts).

Decision hs-d584 (derived versions) and hs-d585 (manual release retirement). hs-d315 is superseded except the `-<commit>` and `-dev` of the build string.
