# Job `e2e`: end-to-end dry run on a toy repo

Tier S4 · Needs: every other Job except `dotfiles` · Read `_common.md` first. Inputs: all of `docs/cli-contract.md`, `skills/cpd-lead/SKILL.md`, `docs/requirements.md`.

## Goal
Prove `cpd` v0 works as a whole on a real herdr session with real pi agents, find the bugs, and produce the first real metrics for tuning defaults. **Fix nothing in `src/`.** Report every defect with a reproduction so the Lead can append fix Jobs.

## Files you own
`test/e2e/**` (scenario scripts and fixtures), `/tmp/cpd-build/e2e/**`, your report. Nothing else.

## Setup
- herdr session `cpd-test` only. `CPD_HOME=/tmp/cpd-build/e2e/home`, with a config per contract §5 except:
  - `concurrency: 2`, `attemptTimeoutMinutes: 2`, `herdrSession: "cpd-test"`;
  - `leadTier: "H2"`, `defaultReviewTier: "H2"`;
  - `tiers` unchanged.

  All Jobs in the toy plan use Tier `H1`.
- Toy repo `/tmp/cpd-build/e2e/toy`:
  - a git repo with `verify.sh` (runs `node --test`) and `.config/wt.toml` (`pre-merge` verify wrapper);
  - one source file `greet.mjs` with tests;
  - a bare repo `/tmp/cpd-build/e2e/origin.git` as `origin`;
  - a fake `gh` on `PATH` for the PR step (records argv, returns canned JSON per contract §13 shapes).
- Lead: `cpd lead start` in a `cpd-test` pane. It loads the pi adapter and plannotator (pinned per config), with `PLANNOTATOR_BROWSER` pointing at a script that records the URL. Plan approval is done over HTTP as in the t4 Recipes.

## Scenario (record a transcript for each step)
1. `cpd workflow init`, `cpd lead attach` (run through the Lead pane), `cpd workflow phase plan`.
2. `cpd job add` ×5:
   - `a` adds `hello()`;
   - `b` needs `a` and adds `bye()`;
   - `c` needs `a` and adds `shout()`;
   - `d` needs `b` and `c` and updates `README.md`;
   - `e` is independent: its Spec tells the Worker to run `cpd ask` "Which greeting language?" before coding.
3. Ask the Lead to call `cpd_plan_review`, approve over HTTP, then `cpd plan lock`. Assert that `cpd watch` is running in a pane.
4. Escalation: when `e` asks, assert the Lead receives the `escalation` message (Lead session JSONL) and that herdr shows the Lead `blocked`. Answer with `cpd answer` from the test harness. Assert `e` continues.
5. Crash and ladder: kill `b`'s Worker pi process once (as in t1 F7). Assert Attempt 1 fails `crashed`, Attempt 2 runs at `H2`, and it succeeds.
6. Timeout: append Job `f` (needs nothing) whose Spec says `sleep 300` before working. Assert `timed_out` → `job-failed` → the Lead message. Then `cpd job cancel f`, and append `g` (a quick replacement).
7. All merged → `all-merged` → `cpd pr open` (fake `gh`, bare origin) → `cpd pr poll` with a canned failing check plus a comment → `pr-feedback` → canned merged → `workflow-done`.
8. Throughout, assert:
   - never more than 2 Worker agents at once (sample `herdr agent list` every 5 s);
   - the Lead Branch history is linear with every Job's commits;
   - the Ledger shows every Attempt with Tier, outcome, cause and metrics;
   - no draft corruption: put a draft in the Lead editor before step 4 and check it at the end.

## Acceptance
1. A report with each step's result, the metrics table (per Attempt: Tier, duration, turns, tokens, cost) and wall time.
2. A defect list: each with steps to reproduce, expected vs actual, suspected module and severity. Suggest fix-Job Specs for each (title, files, acceptance), so the Lead can append them.
3. `test/e2e/run.sh` (or `.ts`) reproduces the scenario non-interactively, guarded by `CPD_IT=1`, and is committed.
4. Cleanup: close all `cpd-test` workspaces you created and kill leftover processes. Leave the `cpd-test` server running.
