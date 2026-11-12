# cpd φ

cpd's mark is **φ**, as pi's is **π**; the mark and its logo font are specified in [docs/mark.md](docs/mark.md).

cpd (Continuous Parallel Delivery) is a framework for how agents work: it enforces its protocols through tooling rather than prose, gates undesired behaviour, and logs every gate as a metric to improve on.

It turns one human ticket into a reviewed pull request: a Lead agent specifies the work, and a swarm of Workers executes it in parallel, each in its own git worktree. One binary, `cpd`, is the CLI, the engine and the cockpit (a terminal UI).

The levels (Jira's words, as an analogy), the cpd objects and their roles are in [CONTEXT.md](CONTEXT.md).

## System dependencies

- `tmux`: hosts the Lead, the Workers and the cockpit
- `git`: worktrees, branches, the merge
- `pi`: the agent harness the Lead and Workers run in
- `gh`: opens and tracks the pull request

## Install and attach

Each item says whether it is on `main` (**merged**) or only decided (**pending**).

### Install on a host (macOS or Linux) (merged)

```sh
sh scripts/install-cpd.sh                 # build cpd and install it
sh scripts/install-cpd.sh --no-follow-up  # write the files, skip the follow-ups
```

The script builds `cpd` in release mode from this checkout and runs `cpd install`, which writes:

- `~/.local/bin/cpd`: the binary
- `~/.local/share/cpd/pi`: the pi adapter

It needs `cargo` on `PATH` (or `$CARGO`, or `~/.cargo/bin`). It is safe to rerun. Without `--no-follow-up`, the install starts the engine (`cpd watch --ensure`).

### Start the server: not working yet

`cpd serve` is the production server of a host: the engine and the WebUI on port 3333, in one process (commands.md §12a). Status on `main`:

- **Merged:** the `cpd serve` command is in the CLI.
- **Broken:** it panics at start, exit 101, for every user. `ServeConfig::default()` resolves the home with neither `CPD_HOME` nor `HOME` (`core/src/serve.rs:38`), before it takes the lock or checks the port. Run by hand, it fails with `resolve default cpd home`.
- **Pending:** the server itself. Even past the panic, `run_serve` stops after its lock and port checks and does not bind 3333, open the store or run the engine's ticks. Until that lands, nothing serves the WebUI.
- **Pending:** the OS user service, launchd on macOS and systemd on Linux. It belongs to the dotfiles services workflow, and this repo has no unit or plist for it.

### Open the WebUI

- **Port 3333, the official interface: pending** (it needs the server above). Open `http://<host>:3333`. No login is needed in the browser (docs/architecture/serve.md, pro-d567). The `/cpd-ws` endpoint accepts a page only from `localhost`, a private IPv4 or IPv6 ULA address, or a `.local` name, on port 3333 or 33333 (web/CONTRACT.md §15.1).
- **Port 33333, UI-only preview: partly merged.** `sh scripts/preview-web.sh on [<worktree>]` runs the Vite dev server of a worktree on `0.0.0.0:33333`, with no fallback port. Today that server hosts its own `/cpd-ws` backend. **Pending (pro-d570):** the preview becomes UI only, forwarding `/cpd-ws` to the production engine on 3333, with no store, binary or engine of its own.

### Upgrade

Merged, by hand: update the checkout to the commit you want (`git pull --ff-only` on `main`), then rerun `sh scripts/install-cpd.sh`.

**Pending (pro-d569):** `cpd upgrade` and the WebUI Upgrade button. The plan is to fetch CI-green `origin/main`, build it in `~/.cpd/upgrade-build`, dry-run the database migration, back up, and roll back automatically when the health check fails. Nothing of this is on `main`: there is no `upgrade` command.

### Repair: `cpd host repair` (merged)

A Manager command (commands.md §9.11). The User runs it from a terminal with no approval. An agent Manager needs a live approval from the User first. It does two things:

1. If the store has diverged migrations, it lists them and asks before it repairs (`-y` skips the question). A repair takes a store backup.
2. It saves the checkout's HEAD as a ref `refs/cpd-host/<stamp>` and its uncommitted diff under `$CPD_HOME/run/cpd-install-backups/`. Then it resets `main` in `~/projects/cpd` to `origin/main`, which discards those uncommitted changes.

It does not repair worktrees.

### State

State lives in `$CPD_HOME`, default `~/.cpd`: the database `cpd.db` (with its `-wal` and `-shm` files), the locks in `locks/` and the runtime files in `run/`. `CPD_HOME` must be an absolute path.

## Layout

- `core/`: package `cpd-core`, the library: model, store, engine, git, tmux host
- `cli/`: package `cpd`, the binary and its command tree; the binary-driven integration tests
- `tui/`: package `cpd-tui`, the cockpit
- `pi/`: the pi adapter (extension and the `cpd-lead` skill)

## Docs

- [docs/mark.md](docs/mark.md): the mark φ and its logo
- [CONTEXT.md](CONTEXT.md): the vocabulary
- [docs/contract.md](docs/contract.md): the normative contract
- [docs/commands.md](docs/commands.md): the command reference
- [docs/implementing.md](docs/implementing.md): where things go in the code
- [docs/architecture.md](docs/architecture.md): the parts and the edges
- [docs/adr/](docs/adr/): decisions
- [docs/archive/cpd1/](docs/archive/cpd1/): cpd 1's docs, history only

## Gate

```sh
pnpm run verify
```

pnpm must be on `PATH`, at the version `package.json` pins (`packageManager`); cpd does not install it.
