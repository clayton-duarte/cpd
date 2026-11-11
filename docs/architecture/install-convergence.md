# Install convergence

Decided by pro-d545 (`app::reload` signals Electron only when `preview-health.json` has `shell_abi` 2 or more; the shell release `app-v0.1.0` sets `CFBundleShortVersionString` to `0.1.0`) and pro-d553 (`cpd install` installs shell release `app-v0.1.0` to `/Applications/CPD.app` on macOS only; Linux, the homeserver, gets none). The normative rules are in `web/CONTRACT.md` §30, "Install convergence". This page states what one `cpd install` leaves on a machine, and where each part is built.

Status: the target. As of this document the code does not do this. Each part below says what the code does today, read from the source, and what the target adds. Nothing here was run on a machine (unverified).

## What one `cpd install` converges

`cpd install --root <checkout>` brings three things to the state below, in addition to the binary and the adapter. Each is idempotent: a rerun on a converged machine changes nothing.

| Part | Target state | Platforms |
|---|---|---|
| Preview worktree | `~/projects/cpd.preview.web` exists, on branch `preview/web`, created from `origin/main` when missing | every machine |
| Prod browser static assets | `<root>/web/dist` is built from the checkout, so the production server on 3333 serves the new code | every machine |
| Shell app | `/Applications/CPD.app` is the shell release `app-v0.1.0` | macOS only |

## Preview worktree

**Target.** When `~/projects/cpd.preview.web` is missing, install creates it: `git worktree add -B preview/web <path> origin/main`, then `pnpm install --frozen-lockfile` in it. When it exists, install does not create it. The default path is `${CPD_WEB_PREVIEW_WORKTREE:-<repo parent>/cpd.preview.web}`, and `~/projects/cpd.preview.web` is that default for a checkout at `~/projects/cpd`.

**Today.** The creation is in `scripts/preview-web.sh`, in `on` and `serve_only` (it runs `git worktree add -B preview/web ... origin/main` when the directory is missing). `cpd install` does not create it. Its step `sync_preview_worktree` (`core/src/install/mod.rs`) only syncs a preview worktree that already exists, to `origin/main`, keeping uncommitted edits as a patch (`web/CONTRACT.md` §28; `docs/contract.md` §3.7). It does nothing when the directory is missing.

**Worktree sync.** An existing preview worktree is still synced to `origin/main` on every install, as `web/CONTRACT.md` §28 states. Creating it when missing does not change that rule.

## Prod browser static assets

**Target.** Install builds the production browser bundle `<root>/web/dist` (`pnpm --dir <root> --filter cpd-web-client run build`, which runs `vite build`) on every machine, so the production server on `0.0.0.0:3333` serves the checkout's code. The build runs on the new checkout, as the Electron bundle does (step 7a).

**Today.** Install builds only the Electron bundle, `web/dist-electron` (`build:electron`, step 7a). Nothing in install runs `vite build`. `web/dist` exists only when someone builds it by hand.

## macOS CPD.app shell install

**Target.** On macOS only, install installs shell release `app-v0.1.0` to `/Applications/CPD.app`. On Linux install does nothing for the shell. Install does not build the shell from the checkout: the shell is the GitHub Release `app-v0.1.0`, built by `scripts/release-shell.sh`.

**Today.** Install does not install CPD.app. `commands.md` §14.4 says so, citing pro-d504 and pro-d505, which pro-d553 supersedes by recency. The install that does exist is `cpd desktop` (`commands.md` §14.4a): when `/Applications/CPD.app` is missing it runs `app::install_and_open_release`, which downloads the latest release with `gh release download` (not the tag `app-v0.1.0`), clears quarantine with `xattr -dr com.apple.quarantine`, and opens the app. `scripts/install-cpd.sh` (9 lines) builds `cpd` and runs `cpd install`; it does not package the app. `commands.md` §14.4's status note says the script still copies a packaged bundle to `~/.local/share/cpd/app`, which the file does not do (unverified against any other checkout; this checkout is the source read).

## Open

- Whether install opens the app after it installs it. `cpd desktop` does; the target does not say.
- What install does when `/Applications/CPD.app` already exists at another version. The target does not say; a rerun on a converged machine must change nothing.
- Whether a failed preview worktree creation or a failed prod build fails the install's exit, or only prints. Not decided; the Electron step's rule (`commands.md` §14.4, step 7a) is the closest precedent.
