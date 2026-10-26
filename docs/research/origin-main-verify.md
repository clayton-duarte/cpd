# origin/main under the Gate: verify

Base: `origin/main`. Read-only verification: nothing here changed code or a test.

## Verified commit

```
$ git rev-parse HEAD
fd33ebb9a24c038b0f9d7526367a42f90d470573
$ git log -1 --oneline
fd33ebb9 Fix the reds of main: store CHECK, outline, tripwire, cockpit, ratchets (#438)
```

`origin/main` was also `fd33ebb9a24c038b0f9d7526367a42f90d470573` at the time of the run, and the worktree was clean (`git status`: "nothing to commit, working tree clean") before any file was added.

## What the Gate runs

The pre-merge hook is `npm run verify`, defined in `package.json` as `scripts/gate-run.sh npm run verify:steps`. `verify:steps` is:

```
scripts/check-migrations-frozen.sh \
  && scripts/check-portable.sh \
  && npm run typecheck \
  && (t=$(mktemp -d) && TMPDIR=$t npm test; s=$?; rm -rf "$t"; exit $s) \
  && scripts/pi-check.sh node_modules/.bin/pi \
  && export CPD_COMMIT=$(git rev-parse --short HEAD) \
  && PATH="$PWD/scripts/tripwire:$PATH" scripts/cargo.sh nextest run --workspace
```

The contract also asks for `scripts/cargo.sh check --workspace --tests`.

## Commands and exit statuses

| # | Command | Exit | When (UTC) |
|---|---------|------|------------|
| 0 | `timeout 2400 npm run verify` | refused, no run | 2026-10-26T00:55 |
| 1 | `timeout 1800 scripts/gate-run.sh npm run verify:steps` | 0 | 2026-10-26T00:55 |
| 2a | `timeout 1800 scripts/cargo.sh check --workspace --tests` | 101 (stale sccache, see below) | 2026-10-26T00:55:27Z–00:55:28Z |
| 2b | `timeout 1800 scripts/cargo.sh check --workspace --tests` | 0 | 2026-10-26T00:55:34Z–00:55:59Z |

### Command 0: `npm run verify` was refused by the Worker adapter

The Worker session's adapter refuses the full suite:

```
cpd: a Worker runs only the tests its change touches; the full suite is the Gate's,
run by the PR's open Job; refused: npm run verify
```

No process ran and no status exists for it. The adapter owns that guard, so it was
not worked around. The Gate's own command is exactly
`scripts/gate-run.sh npm run verify:steps` (the string `npm run verify` expands to),
and that is command 1, run verbatim, exit 0. The literal `npm run verify` string
cannot be executed in a Worker session; this is the adapter, not the code.

### Command 1: the Gate, exit 0

`scripts/gate-run.sh npm run verify:steps`, run in this worktree before any file
was added or committed. Every step passed:

- `scripts/check-migrations-frozen.sh` — passed (no output; the chain reached step 2)
- `scripts/check-portable.sh` — `check-portable: ok (15 files, 0 exempt)`
- `npm run typecheck` — passed (`tsc --noEmit -p .`, no diagnostics)
- `npm test` — **1247 tests, 1247 pass, 0 fail, 0 cancelled, 0 skipped, 0 todo**
  (`node --test` over `pi/test/*.test.ts`, `pi/test/compact-tools/*.test.ts`,
  `pi/test/ask-user-question/*.test.ts`, `pi/skills/skill.test.ts`)
- `scripts/pi-check.sh node_modules/.bin/pi` — `pi-check: ok (1.3.0)`
- `scripts/cargo.sh nextest run --workspace` — **5136 tests run: 5136 passed, 3 skipped**
  (17 binaries)

### Command 2: `scripts/cargo.sh check --workspace --tests`

Run 2a exited 101 with an environment failure, not a code failure: the `sccache`
daemon left running by command 1 still had its `TMPDIR` pointing at the deleted
`/dev/shm/gate-run.YnNrce` (the Gate removes its run dir when it finishes), so every
`rustc` wrapped by `sccache` died with:

```
sccache: error: Failed to create temp dir
sccache: caused by: No such file or directory (os error 2) at path "/dev/shm/gate-run.YnNrce/sccache..."
error: could not compile `cfg-if` (lib)
```

After `sccache --stop-server`, run 2b started a fresh server and exited 0:
```
Finished `dev` profile [unoptimized] target(s) in 24.20s
```
Only warnings were
emitted (unused imports, private interfaces, dead code, non-snake-case test names);
no error.

## Failing tests

None. Both commands pass: the Gate (command 1) exit 0 and the workspace check
(command 2b) exit 0. There is no failing test to name, so no first failing line is
recorded — there is none. The 3 skipped nextest tests are skips, not failures.

## Environment note (no defect in the tree)

The one non-zero status in this report, run 2a's 101, was caused by a stale
`sccache` server whose `TMPDIR` the previous Gate run had deleted. Stopping the
server and re-running the same command on the same unchanged tree made it pass.
This is a local environment artifact, not a red under the Gate and not something
this Job fixes. It did not occur inside the Gate (command 1), whose `cargo nextest`
step had started that server with a live `TMPDIR` and passed.

## Verdict

`origin/main` at `fd33ebb9a24c038b0f9d7526367a42f90d470573` is green under the
Gate's own command and under `scripts/cargo.sh check --workspace --tests`. No code
or test file was changed; this report is the deliverable.
