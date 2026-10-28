# cpd φ

cpd's mark is **φ**, as pi's is **π**; the mark and its logo font are specified in [docs/mark.md](docs/mark.md).

cpd (Continuous Parallel Delivery) turns one human ticket into a reviewed pull request: a Lead agent specifies the work, and a swarm of Workers executes it in parallel, each in its own git worktree. One binary, `cpd`, is the CLI, the engine and the cockpit (a terminal UI).

The levels (Jira's words, as an analogy), the cpd objects and their roles are in [CONTEXT.md](CONTEXT.md).

## System dependencies

- `tmux`: hosts the Lead, the Workers and the cockpit
- `git`: worktrees, branches, the merge
- `pi`: the agent harness the Lead and Workers run in
- `gh`: opens and tracks the pull request

## Install and attach

```sh
sh scripts/install-cpd.sh   # ~/.local/bin/cpd and ~/.local/share/cpd/pi
cpd tui --attach            # open the cockpit
sh scripts/preview-tui.sh on   # preview this checkout's cockpit views (the status line shows 'preview: <worktree> <sha>')
sh scripts/preview-tui.sh off  # back to the installed cpd
```

State lives in `$CPD_HOME` (default `~/.cpd`).

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
- [docs/adr/](docs/adr/): decisions
- [docs/archive/cpd1/](docs/archive/cpd1/): cpd 1's docs, history only

## Gate

```sh
npm run verify
```
