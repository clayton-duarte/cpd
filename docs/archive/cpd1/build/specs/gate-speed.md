# Job `gate-speed`: make `./verify.sh` fast again without weakening it

Tier S3 · Needs: `ledger-hardening` (it edits the same ledger tests) · Contract sections: §1 (tests), §8.

## Goal
`./verify.sh` now takes about 8 minutes alone and 20 minutes when other Gates run at the same time, and every Attempt runs it at least four times (Worker, submit, rebase merge, post-merge). Almost all of the time is in tests that call the real `bd` (single tests take 100–300 s under load; `bd` costs roughly 0.4–1 s per call). Cut the Gate's wall time, run alone on this Mac, to **under 4 minutes**, without removing, skipping or weakening any test.

## Files you own
`test/helpers/**`, `test/ledger/**`, `test/commands/**` (only to switch tests to faster fixtures; assertions stay identical), `src/ledger/bd.ts` and `src/ledger/workflow.ts` (only to cut redundant `bd` calls, with identical behaviour and output).

## Requirements
- Measure first: wall time of `./verify.sh` alone (3 runs), the 15 slowest tests, and the number of `bd` calls per slow test (count them with a fake `bd` wrapper on `PATH` that logs and delegates to the real one). Put the table in the report.
- Then apply the cheapest changes with the most effect, for example:
  - initialise one Ledger once per test file (or copy a pre-initialised template directory) where a test does not test `init` itself;
  - remove repeated lookups (such as finding the Workflow issue with `bd list` on every call) by reading them once per Ledger handle;
  - batch writes where `bd batch` already gives the same result.
- Every test keeps its name and its assertions. No test is skipped, deleted or moved behind `CPD_IT`.
- Production behaviour and command output stay identical; the existing tests prove it.

## Acceptance
1. Before and after table: `./verify.sh` wall time alone (3 runs each), median under 4 minutes after.
2. The number of tests and their names are identical before and after (diff of `node --test` TAP test names, attached).
3. `./verify.sh` passes.

## Out of scope
Changing `verify.sh`, `package.json` or the test runner flags (protected Gate files); changes to `bd` itself.
