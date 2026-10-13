# Job `dotfiles`: install `cpd` and its config on every Mac through the dotfiles

Tier S2 · Needs: `config-tiers` · Read `_common.md` first, but this Job works in **`~/dotfiles`**, not in the cpd repo. Read `~/dotfiles/AGENTS.md` in full and follow it: work in a dotfiles worktree, declare machine differences in `machines.yaml`, use `run_onchange` scripts, never `chezmoi apply --force`, and run the parity test before and after.

## Goal
Make `cpd` and its machine config travel to both Macs the way the rest of the setup does.

## Facts
- cpd repo remote: **`https://github.com/clayton-duarte/cpd.git`** (private).
- Local clone path: `~/projects/cpd`.
- Entry point: `bin/cpd.mjs` (Node 22, `npm ci` installs deps).
- Config schema: `docs/cli-contract.md` §5 in the cpd repo.
- Machine values: macbook-air concurrency **3**, work Mac (`ml-k977qw1q2t-duarte`) concurrency **8**. Every other value uses the §5 example values.
- Machines that get cpd: the two Macs only (they run pi and herdr).
- **Verified on the work Mac on 2026-10-13 over `ssh mac-pro`:** node 22.23.3, pi 1.1.0, bd 1.3.1, wt 0.80.0, herdr 0.9.3, gh, chezmoi 2.73.1; 14 cores, 48 GB. Its pi catalog has `github-copilot/claude-opus-5.5`, `claude-sonnet-5.5` and `claude-haiku-4.5`, so the §5 `tiers` values are valid there unchanged (its `provider: litellm` in `machines.yaml` is for Hermes, not pi).
- **The cpd code is not on `main` yet** (it is on branch `cpd/v0` until the v0 PR is merged). The script must not fail when `~/projects/cpd/package.json` is missing: print one notice and skip `npm ci` and the link. Once `main` has the code (or the User checks out another branch by hand), the next apply installs it.
- On this MacBook Air, `~/.local/bin/cpd` is currently a bootstrap symlink to `~/projects/cpd.cpd-v0/bin/cpd.mjs` that the running build depends on. Do not touch it; the Lead applies your change only after the build.

## Files you own (in `~/dotfiles`, via a worktree)
- `home/.chezmoidata/machines.yaml`: add `cpd` to `modules` and `cpd: {concurrency: N}` for the two Macs.
- `home/dot_cpd/config.json.tmpl` → `~/.cpd/config.json`, rendered with the machine's concurrency, gated on the `cpd` module.
- `home/.chezmoiscripts/run_onchange_after_75-cpd.sh.tmpl`:
  - if `~/projects/cpd` is absent, `git clone https://github.com/clayton-duarte/cpd.git ~/projects/cpd`; otherwise leave it alone (never pull, reset or checkout, per the dotfiles rule for tool repos);
  - then `npm ci` in it when `node_modules` is missing or `package-lock.json` changed (record a checksum marker);
  - then link `~/.local/bin/cpd` → `~/projects/cpd/bin/cpd.mjs`.
  - Gated on the `cpd` module. Check `~/.local/bin` is on `PATH` in the managed shell config, and say where.
- `.chezmoiignore` entries if needed for non-cpd machines.
- Tests in `tests/` if the dotfiles test suite has a pattern for scripts and templates.

## Acceptance
1. `chezmoi execute-template` of the config template renders valid JSON matching §5 for both Macs (show both renders, with the hostname overridden as the existing tests do). The rendered script is `bash -n` clean.
2. `.venv/bin/python -m pytest tests/test_chezmoi_parity.py -q` has the same result before and after (the pre-existing `test_no_secrets_in_source_state` failure stays the only failure).
3. On this machine: `chezmoi apply -k` applies only this Job's targets and scripts, plus any pre-existing pending ones. **Do not run apply.** Instead show `chezmoi diff`, then stop: the Lead applies after review.
4. The branch is committed in the dotfiles worktree. Do not merge it into `main` (the Lead merges after review). Report the worktree path and branch.

## Out of scope
The cpd code; pi settings (the Lead starts with `cpd lead start`, which passes `-e` flags, so no pi settings change is needed); the work Mac itself (it converges on its next sync).
