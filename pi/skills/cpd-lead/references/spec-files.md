# Spec files, sizing, Tiers and Dependencies

A Worker is a fresh session that sees only its Spec. Whatever it needs must be in the spec file.

## The spec file

The file passed as `--spec` when you add a Job is JSON with exactly these fields, all required, and no others (an unknown field is refused with `spec_invalid`); a test Job's also has `tests` and `test_files` ([Test Jobs](#test-jobs)):

```json
{
  "goal": "Add a /health endpoint that returns 200 and {\"status\":\"ok\"}.",
  "context": "The server is src/server.ts (Express 5). Routes are registered in src/routes/index.ts. Accepted assumption A2: no auth on /health.",
  "touches": ["src/routes/health.ts", "src/routes/index.ts", "test/routes/health.test.ts"],
  "acceptance": [
    "GET /health returns 200 with body {\"status\":\"ok\"}",
    "`timeout 300 node --test test/routes/health.test.ts` passes",
    "The repo's quick check passes"
  ],
  "out_of_scope": ["Do not change the existing routes", "Do not add a dependency"]
}
```

| Field | What it holds |
|---|---|
| `goal` | One outcome, in the imperative. |
| `context` | Everything the Worker needs and cannot ask the User for: facts, paths, names, signatures, how to run things, decisions from the interview and the Context's records, the accepted assumptions that apply. Paths are repo-relative, never the primary checkout of the repo. |
| `touches` | Exact paths the Worker may change, at least one. New files included. The Worker may touch nothing else. Paths are repo-relative, never the primary checkout of the repo. |
| `acceptance` | At least one item, each checkable by a command or an observable result. Test items name the targeted tests: the test files or modules of what the Job changes plus its new tests (for example `scripts/cargo.sh nextest run -p cpd --test plan_overlap`, `node --test pi/test/updates.test.ts`). Never a full unfiltered run (the whole workspace, `npm test` of everything): the full suite is the Gate's, run by the PR's open Job. |
| `out_of_scope` | The adjacent temptations to refuse. May be empty, rarely should be. |
| `resources` | Optional. A list of one or more distinct names of declared resources (an e2e port, for example; declared in the host, owner or repo policy, each with `slots`, default 1) that the Job's run needs, one slot of each; absent means none. Find the names with `cpd policy show --pr <key>` (the `resources` domain: `names`, and each name's `scope` and `slots`) or `cpd resource list` run in the PR's Lead worktree; a name the repo does not declare is `resource_undeclared`, and a review Job takes none (`spec_invalid`). The Job waits `queued` while any listed resource has no free slot, and `cpd job show` has `waiting_for` while it waits; you are not told otherwise; `workers`, the Worker slots, is waited for the same way and is never listed (`spec_invalid`). A dev server you run for the User is prefixed with `cpd resource run <name> --`, which takes any free slot. |

Write the text in GFM only: tables, lists, code and Mermaid. Footnotes render as literal text and raw HTML is not rendered, so the User would see them broken in the Plan.

The Worker's submit runs only the repo's quick check (its `pre-commit` hook) and, for a test or implementation Job, the named tests ([Test Jobs](#test-jobs)). The Gate (its `pre-merge` hook) runs later, on the whole Lead Branch, in the submit of the PR's open Job, so an acceptance item that needs the Gate must name the command itself.

## Size

- An Attempt gets `attempt_minutes` of active time (the configured `attempt_minutes`); the timeout is that value. Size a Job to fit **well inside it** (about 150 turns), including worktree setup and dependency install. A Job that needs more fails and costs a retry.
- A Job is one outcome that one Worker can finish and verify. Two independent outcomes are two Jobs.
- Prefer many small Jobs that run in parallel. Do not split below a unit that has its own acceptance check.
- A Job key matches `^[a-z][a-z0-9-]{0,31}$` and is unique in the Plan.

## Bounded commands

Put these in the `context` of every Spec (they are a Worker's only rules):

- Commit a skeleton within 10 minutes, then at least every 20 minutes.
- Wrap every measurement and test run in `timeout` (for example `timeout 300 node --test test/routes/health.test.ts`), and run only the tests your change touches, never the full suite.
- Write no synthetic benchmarks.
- Never start a command that does not exit: `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run`.

## Dependencies

- Add `--needs <k1,k2>` when a Job changes files another Job changes, imports what another Job produces, or tests another Job's behaviour. The exception is the PR shape's red tests: they come first, `--needs` only the docs Job, and the implementation `--needs` them (an implementation Job must need a test Job of its PR, `test_job_required`; a `--kind verify` Job must need an implementation Job of its PR, `impl_job_required`). A PR's open and update Jobs are cpd's and have no Dependencies: the open Job waits for every other Job of its PR by its own rule, so nothing needs it. A Job starts only after its Dependencies are Merged and branches from the Lead Branch that contains them.
- Jobs with disjoint `touches` and no data flow between them need no Dependency. Shared files such as lockfiles, `package.json`, config and index or route registries count as shared.
- Dependencies cannot form a cycle (`dependency_cycle`), cannot name an unknown Job (`needs_unknown`) and cannot name a cancelled or failed Job (`needs_unavailable`).

## Tiers

Format `<model><effort>`: models `O` (opus), `S` (sonnet), `H` (haiku), `G` (gemini, auxiliary only); effort 0–5, or `GX` for G. An invalid Tier is refused when you add the Job (`tier_invalid`, with the valid efforts in `error.details.efforts`). `cpd tiers` prints the efforts each model accepts. **A Worker Job's Tier is `H0`–`H3` or `S0`–`S3` only** (pro-d82); a Worker's ladder goes H0>H1>H2>H3>S1>S2>S3 and ends at `S3`. `cpd tiers` prints the efforts each model accepts; the catalog can cut further. G (written `GX`, no effort levels) is never a Job's Tier: it is for auxiliary one-shot pi runs only (the distiller, title writer, and similar helpers), never a Worker's, Lead's or Manager's.

| Work | Tier | Why |
|---|---|---|
| Coding, tests, refactors | `S1` | The User's default. Start low: each failed Attempt that the ladder retries (`crashed`, `no_submit`, `merge_conflict`) raises effort by one on the next Attempt, up to `retry_cap` retries (default 2, so 3 Attempts). |
| Docs, config, mechanical edits | `H3` | Cheap and enough. |
| A PR's open and update Jobs | `open_tier` (default `S1`) | cpd sets it, you never choose it; if one fails, retry higher with `cpd job retry <k> --tier <T>`. |

## A feedback fix Job

On an open PR a fix is one `work` Job (`--tier S1`, no `--kind`, no `--draft`): its Spec carries the comment text, file and line, the failing log lines and the exact fix, and, when behaviour changes, an acceptance item naming the test the Worker writes first in the same Job. Its merge pushes the pull request ([Addressing](../SKILL.md#addressing)).

## Test Jobs

A PR that changes behaviour has a test Job and an implementation Job after its docs Job, and, for an end-to-end or other check that changes no product behaviour, a `verify` Job after the implementation Job (plain Spec, no `tests`) ([the PR's shape](../SKILL.md#dividing)). A different Worker writes each, so the tests are not fitted to the code.

- **A test Job's spec file** has two more fields, both required once the Job is not a stub, and refused for every other kind: `tests`, the named tests (one or more distinct one-line strings, the names the repo's test command takes), and `test_files`, the files that hold them (repo-relative paths or, ending in `/`, directory prefixes; no `..`, no leading `/`). Name the tests exactly: with no `list` command below, a name that matches no test can read as a red test. Its `touches` includes those files and the stubs that make the branch build.

  ```json
  {"goal": "Write the red tests for the /health endpoint, with stubs.", "context": "…", "touches": ["test/routes/health.test.ts", "src/routes/health.ts"], "acceptance": ["…"], "out_of_scope": ["…"],
   "tests": ["health returns 200", "health body is ok"], "test_files": ["test/routes/health.test.ts"]}
  ```

  Its submit builds the branch and runs the named tests, and is refused unless they fail for a reason other than a build error (`tests_build_failed`, `tests_not_red`; `test_files_missing` when a `test_files` entry matches nothing the Job changed).
- **An implementation Job names no tests.** Its tests are the union of the `tests` of the test Jobs it needs directly (derived, so the tests it must pass are the ones that were red). Its submit is refused unless they pass (`tests_failed`). Its Spec's `context` says which test Jobs it builds on.
- **The red tests are guarded.** The implementation Job may not change a path matched by the `test_files` of the test Jobs it needs (`tests_tampered`), unless you approve the path with `cpd job approve-tests <job> --path <p> --reason "<why>"`. A Worker that needs a test changed asks you with `cpd ask`; the decision is yours ([Escalations](../SKILL.md#escalations)).
- **The repo's test commands** are the `[test-commands]` table (formerly `[tests]` before the policy file migration) of the repo's policy file at `$CPD_HOME/policy/repos/<owner>/<name>.toml`, which the Lead drafts and the User approves:

  ```toml
  [test-commands]
  build = "<compile what the named tests need; \"true\" when nothing builds>"
  list  = "<optional: exit zero when the one test named by {{ test }} exists>"
  run   = "<run the named tests, {{ tests }}: exit non-zero when one fails>"
  ```

  cpd tells a build error from a red test by which of `build` and `run` fails, and reads exit statuses only. With `list`, a name that matches no test is refused (`tests_not_found`) before `run`; a repo whose runner exits non-zero on an unknown name needs it. A repo with no policy file is fine (it inherits the owner's policy); a `tdd` PR that needs `[test-commands]` is refused `tests_unconfigured`: a docs-only or config-only PR is made with `--shape docs` or `--shape config`.

## Research Jobs

Grounding that needs more than reading (a large codebase survey, comparing libraries, checking an API against its source) goes to a Worker as an ordinary Job.

- Spec: the question as `goal`; where to look and what a good answer contains as `context`; `touches` is the one Markdown file for the findings (for example `docs/research/<topic>.md`); `acceptance` says what the file must answer and cite. Tier `S1` to `S3`.
- Jobs run only after the lock, so a research Job cannot ground the Plan that contains it. Put it in the Plan first and add the follow-up Jobs as drafts right after the lock (`job add --draft`), listing them in its `context`. When it is Merged (`job-merged`), revise the drafts with what it found, then start them with `cpd_plan_review` (with a `title`). Tell the User what the batch holds. A follow-up that is its own PR is added right after the lock with `cpd pr add <key> --repo <path> --title "<title>" --needs <research-pr>` so it waits on the research PR's merge, with its stub drafts; the research findings are read from main when it starts.
- A research Job obeys the [bounded commands](#bounded-commands): it commits its findings file early and measures only under `timeout`, with real data, never a synthetic benchmark.
- If the question is cheap to answer by reading, read instead.
