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
  "out_of_scope": ["The existing routes stay unchanged", "No new dependency"]
}
```

| Field | What it holds |
|---|---|
| `goal` | One outcome, in the imperative. |
| `context` | Everything the Worker needs and cannot ask the User for: facts, paths, names, signatures, how to run things, decisions from the interview and the Context's records, the accepted assumptions that apply. Paths are repo-relative, never the primary checkout of the repo. |
| `touches` | Exact paths the Worker may change, at least one. New files included. The Worker may touch nothing else. Paths are repo-relative, never the primary checkout of the repo. |
| `acceptance` | At least one item, each checkable by a command or an observable result. Test items name the targeted tests: the test files or modules of what the Job changes plus its new tests (for example `scripts/cargo.sh nextest run -p cpd --test plan_overlap`, `node --test pi/test/updates.test.ts`). Never a full unfiltered run (the whole workspace, `npm test` of everything): the full suite is the Gate's, run by the Workflow's open Job. |
| `out_of_scope` | The adjacent temptations to refuse. May be empty, rarely should be. |
| `resources` | Optional. A list of one or more distinct names of declared resources (an e2e port, for example; declared in the host, owner or repo policy, each with `slots`, default 1) that the Job's run needs, one slot of each; absent means none. Find the names with `cpd policy show --workflow <key>` (the `resources` domain: `names`, and each name's `scope` and `slots`) or `cpd resource list` run in the Workflow's Lead worktree; a name the repo does not declare is `resource_undeclared`, and the Job kinds that take none say so in their own section (`spec_invalid` otherwise). The Job is a tenant of each listed resource from its first Attempt until it is merged or cancelled, and a failed Job keeps them all (retry or cancel it at once). While any listed resource has no free slot the Job stays `queued` and is skipped, not blocked: nothing is reserved for it, and E3 serves it before the later queue, first skipped first served; `cpd job show` has `waiting_for` while it waits and you are not told otherwise. `workers`, the Worker slots, is acquired with them, all or none, and is never listed (`spec_invalid`). A Job that needs one resource waits a bounded time; a Job that needs two or more is not guaranteed to start, so list only what the run needs. A dev server you run for the User is prefixed with `cpd resource run <name> --`, which takes any free slot. |

Write the text in GFM only: tables, lists, code and Mermaid. Footnotes render as literal text and raw HTML is not rendered, so the User would see them broken in the Plan.

The Worker's submit runs only the tamper check and, for a test or implementation Job, the named tests ([Test Jobs](#test-jobs)). The repo's quick check (its `pre-commit` hook) runs at the Job's merge, and the Gate (its `pre-merge` hook) runs later, on the whole Lead Branch, at the merge of the Workflow's open Job, so an acceptance item that needs the Gate must name the command itself.

## Size

- An Attempt gets `attempt_minutes` of active time (the configured `attempt_minutes`); the timeout is that value. Size a Job to fit **well inside it**, including worktree setup and dependency install. A Job that needs more fails and costs a retry.
- A Job is one outcome that one Worker can finish and verify. Two independent outcomes are two Jobs.
- Prefer many small Jobs that run in parallel. Split down to a unit that has its own acceptance check, no further.
- A Job key matches `^[a-z][a-z0-9-]{0,31}$` and is unique in the Plan.

## Bounded commands

Put these in the `context` of every Spec (they are a Worker's only rules):

- Commit a skeleton within 10 minutes, then at least every 20 minutes.
- Wrap every measurement and test run in `timeout` (for example `timeout 300 node --test test/routes/health.test.ts`), and run only the tests your change touches, never the full suite.
- Write no synthetic benchmarks.
- Never start a command that does not exit: `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run`.

## Dependencies

- Add `--needs <k1,k2>` when a Job changes files another Job changes, imports what another Job produces, or tests another Job's behaviour. The exception is the Workflow shape's red tests: they come first, `--needs` only the docs Job, and the implementation `--needs` them (an implementation Job must need a test Job of its Workflow, `test_job_required`; a `--kind verify` Job must need an implementation Job of its Workflow, `impl_job_required`). A feedback fix Job needs nothing: cpd chains it after the Workflow's open Job and previous fixes, so a failed fix must be retried or cancelled before the next is added (`needs_unavailable`). A Workflow's open and update Jobs are cpd's and have no Dependencies: the open Job waits for every other Job of its Workflow by its own rule, so nothing needs it. A Job starts only after its Dependencies are Merged and branches from the Lead Branch that contains them.
- Jobs with disjoint `touches` and no data flow between them need no Dependency. Shared files such as lockfiles, `package.json`, config and index or route registries count as shared.
- Dependencies cannot form a cycle (`dependency_cycle`), cannot name an unknown Job (`needs_unknown`) and cannot name a cancelled or failed Job (`needs_unavailable`).

## Tiers

Format `<letter><effort>`: letters `O` (opus), `S` (sonnet), `H` (haiku), `G` (gemini), ranked G < H < S < O; effort `0`–`4` (a digit only where the model offers that effort; digits may skip), or `X` for any model (the effort order is 0 < X < 1 < 2 < 3 < 4, so X sends no effort; air-d16, air-d17, air-d21); effort 5 is never valid. An invalid Tier is refused when you add the Job (`tier_invalid`, with the valid efforts in `error.details.efforts`). `cpd tiers` prints the efforts each model accepts. **A Worker Job's Tier is inside the worker range**, from `worker_tier` up to just below `lead_tier` (contract §9 "Roles and ranges"). Read that range from `cpd tiers` and the config, never from a fixed list; the explore kinds have their own ranges ([Explore Jobs](#explore-jobs)). A Worker's ladder climbs to the next valid effort inside the letter (contract §9 'Next rung') and stops at the range's ceiling. `cpd tiers` prints the efforts each model accepts; the catalog can cut further. A Lead's Tier is in the lead range (`lead_tier` up to just below `manager_tier`), a Manager's in the manager range (`manager_tier` up to the top valid rung); both are set by the config, not by you. `GX` is the auxiliar's Tier (the distiller, `context_distill_tier`, default `GX`); G is a letter like the others, and a Job's Tier of letter G is refused unless the worker range holds it.

| Work | Tier | Why |
|---|---|---|
| Coding, tests, refactors | `S1` | The User's default. Start low: each failed Attempt that the ladder retries (`no_submit`) raises effort by one on the next Attempt, and a `crashed` or `merge_conflict` Attempt is retried at the same Tier, up to `retry_cap` retries (default 2, so 3 Attempts). A provider error retries the same Tier up to 3 times first and uses no try. A `gate_failed` is retried once at the same Tier and uses a try. |
| Docs, config, mechanical edits | `H3` | Cheap and enough. |
| A Workflow's open and update Jobs | `worker_tier` (default `H3`) | cpd sets it, you never choose it; if one fails, retry higher with `cpd job retry <k> --tier <T>`. |

## A feedback fix Job

On an open Workflow a fix is one `work` Job (`--tier S1`, no `--kind`, no `--draft`): a feedback round puts every fix of the round in it; cpd chains it after the Workflow's open Job and previous fixes, so you give no `--needs` for the chain. The Job's Spec carries the comment text, file and line, the failing log lines and the exact fix, and, when behaviour changes, an acceptance item naming the test the Worker writes first in the same Job. A failed fix must be retried or cancelled before the next is added (`needs_unavailable`). Its merge pushes the pull request ([Addressing](../SKILL.md#addressing)). The pull request body or title is never a Job: the Lead's, written with `cpd workflow describe <key> --file <body.md>`.

## Test Jobs

A Workflow that changes behaviour has a test Job and an implementation Job after its docs Job, and, for an end-to-end or other check that changes no product behaviour, a `verify` Job after the implementation Job (plain Spec, no `tests`) ([the Workflow's shape](../SKILL.md#splitting)). A different Worker writes each, so the tests are not fitted to the code.

- **A test Job's spec file** has two more fields, both required once the Job is not a stub, and refused for every other kind: `tests`, the named tests (one or more distinct one-line strings, the names the repo's test command takes), and `test_files`, the files that hold them (repo-relative paths or, ending in `/`, directory prefixes; no `..`, no leading `/`). Name the tests exactly: with no `list` command below, a name that matches no test can read as a red test. Its `touches` includes those files and the stubs that make the branch build.

  ```json
  {"goal": "Write the red tests for the /health endpoint, with stubs.", "context": "…", "touches": ["test/routes/health.test.ts", "src/routes/health.ts"], "acceptance": ["…"], "out_of_scope": ["…"],
   "tests": ["health returns 200", "health body is ok"], "test_files": ["test/routes/health.test.ts"]}
  ```

  Its submit builds the branch and runs the named tests, and is refused unless they fail for a reason other than a build error (`tests_build_failed`, `tests_not_red`; `test_files_missing` when a `test_files` entry matches nothing the Job changed).
- **An implementation Job names no tests.** Its tests are the union of the `tests` of the test Jobs it needs directly (derived, so the tests it must pass are the ones that were red). Its submit is refused unless they pass (`tests_failed`). Its Spec's `context` says which test Jobs it builds on.
- **The red tests are guarded.** The implementation Job may not change a path matched by the `test_files` of the test Jobs it needs (`tests_tampered`), unless you approve the path with `cpd job approve-tests <job> --path <p> --reason "<why>"`. A Worker that needs a test changed asks you with `cpd ask`; the decision is yours ([Escalations](../SKILL.md#escalations)).
- **The repo's test commands** are the `[test-commands]` table of the repo's policy file at `$CPD_HOME/policy/repos/<owner>/<name>.toml`, which the Lead drafts and the User approves:

  ```toml
  [test-commands]
  build = "<compile what the named tests need; \"true\" when nothing builds>"
  list  = "<optional: exit zero when the one test named by {{ test }} exists>"
  run   = "<run the named tests, {{ tests }}: exit non-zero when one fails>"
  ```

  cpd tells a build error from a red test by which of `build` and `run` fails, and reads exit statuses only. With `list`, a name that matches no test is refused (`tests_not_found`) before `run`; a repo whose runner exits non-zero on an unknown name needs it. A repo with no policy file is fine (it inherits the owner's policy); a docs-only or config-only Workflow is made with `--shape docs` or `--shape config`.

## Research Jobs

A Research Job is an ordinary Workflow Job that writes `docs/research/<topic>.md` and goes through review; it is not an explore Job (`--kind scout|research`, no Workflow, queued at once, artefact in `$CPD_HOME/run/<plan>/explore/<job-key>.md`: [Explore Jobs](#explore-jobs)). Grounding that reads a codebase, compares libraries or checks an API against its source goes to an explore Job as one.

- Spec: the question as `goal`; where to look and what a good answer contains as `context`; `touches` is the one Markdown file for the findings (for example `docs/research/<topic>.md`); `acceptance` says what the file must answer and cite. Tier letter `S`, effort 1 or above or `X` (air-d16, air-d17, air-d21), up to the worker range's ceiling.
- Put it in the Plan first and add the follow-ups right after the lock queued with `--needs <research-job>` (same Workflow), listing them in its `context`: they wait on it, never as drafts. Tell the User what the batch holds. A follow-up that is its own Workflow is added right after the lock with `cpd workflow add <key> --repo <path> --title "<title>" --needs <research-workflow>` so it waits on the research Workflow's merge, and its Jobs are added queued with `--jobs <file.json>`; the research findings are read from main when it starts. This is the exception to stacking ([Scoping](../SKILL.md#scoping)): the follow-up reads what is on `main`, so it does not stack (`--base`) on the research Workflow.
- A research Job obeys the [bounded commands](#bounded-commands): it commits its findings file early and measures only under `timeout`, with real data, never a synthetic benchmark.
- Promoting an explore finding into the repo (a `docs/research/*.md` file) is an ordinary docs Job the Lead adds ([Explore Jobs](#explore-jobs)).

## Explore Jobs

The explore kinds are `scout` and `research`; a Job of one is an explore Job. A **scout** answers one question from this machine only (Tier letter `H`, effort 1 or above or `X` (air-d16, air-d17, air-d21), up to the worker range's ceiling); a **research** Job may also reach outside it (web, curl, attached MCP servers, registries, upstream docs; Tier letter `S`, effort 1 or above or `X` (air-d16, air-d17, air-d21), up to the worker range's ceiling). They answer for you from a Worker's session, so the material stays out of yours ([Exploring](../SKILL.md#exploring)). An explore Job belongs to the Plan, not to a Workflow: add it with `--kind scout|research` (`--kind scout` or `--kind research`) and no `--workflow`, and with no `--draft`. It is `queued` at once, with no review and no lock.

The file passed as `--spec` has exactly these four fields, all required, and no others (`spec_invalid`, exit 2, with the JSON path, for `touches`, `tests`, `test_files` or `resources`):

| Field | What it holds |
|---|---|
| `goal` | The one question, in one sentence. |
| `context` | Where to look and what a good answer contains. Absolute machine paths are allowed here: the repo-relative rule is waived for explore Jobs. The rules below go here too. |
| `acceptance` | What the file must answer and cite, one checkable item each. |
| `out_of_scope` | The adjacent temptations to refuse. May be empty. |

cpd stores the artefact as the Job's `touches`: `$CPD_HOME/run/<plan>/explore/<job-key>.md`. You name no file.

Rules to put in the `context` of every explore Spec (they are the Worker's only rules):

- **One question per Job.** A second question is a second explore Job.
- **Scoped paths.** Name the paths the Worker may read, exactly. Never a search from `/`, `/Users`, `/home`, `~` or `$HOME`.
- **The 30 s bound.** Every shell call sets a `timeout` of 30 s or less, except `cpd ask`, which the Worker runs alone in the foreground with the `timeout` input at 7200 (its Spec says how). Never start a command that does not exit: `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run`.
- **Write only the artefact.** No file in a repo, no commit.
- **The summary is at most 1500 characters** (counted as characters): `summary_too_long` (exit 3, never truncated) refuses a longer one, and the Worker shortens it. The summary reaches you; the file does not.

`acceptance` says what the file must answer and cite: for this machine, each fact as path and line; for the outside, each claim as a URL. Two examples, one scout and one research Job, each a complete Spec file:

```json
{
  "goal": "Which command runs the cpd Rust tests, and which CI job runs it?",
  "context": "Read only /Users/<you>/projects/cpd/scripts and /Users/<you>/projects/cpd/.github (the checkout on this machine). Answer: the command a developer runs for the Rust tests, the script behind it, and the CI job that calls it, each cited as path:line. Every shell call sets a timeout of 30 s or less. Never search from /, /Users, /home or ~. One question only: do not survey the rest of the repo. Write nothing except your artefact. The summary is at most 1500 characters.",
  "acceptance": [
    "The file names the developer command, the script behind it and the CI job that runs it",
    "Each fact cites its file and line, for example scripts/cargo.sh:12"
  ],
  "out_of_scope": ["The repo stays unchanged", "The test suite is not run"]
}
```

```json
{
  "goal": "Does zod 4 still export z.string().email(), and what do its release notes say about it?",
  "context": "Upstream sources only: the docs at https://zod.dev and the release notes at https://github.com/colinhacks/zod/releases. Use curl on each call, with a timeout of 30 s or less. Answer: whether the call exists in the current major version, the version that changed it, and each claim cited by URL with the date the page was read. Never search from /, /Users, /home or ~: this question needs no file on this machine. One question only. Write nothing except your artefact. The summary is at most 1500 characters.",
  "acceptance": [
    "The file states whether the call exists in the current major version, with the URL that says so",
    "The file names the version that changed it, or says that no source names one"
  ],
  "out_of_scope": ["The repo stays unchanged", "No package is installed"]
}
```

Add the Job and read its summary when it merges (`job-merged`). Its file stays where the Worker wrote it, `$CPD_HOME/run/<plan>/explore/<job-key>.md`: the Lead never reads it (the adapter refuses it). Cite that path in the Spec of each Job that needs the finding, so its Worker reads it.

- **Dependencies.** An explore Job may need only other explore Jobs, and no other Job may need one (`explore_needs_mixed`, exit 3).
- **Tier.** A scout is letter `H` and a research Job letter `S`, each at effort 1 or above or `X` (air-d16, air-d17, air-d21), and at most the worker range's ceiling (read it from `cpd tiers` and the config); any other Tier is `tier_not_allowed` (exit 2, with the kind's range in `details`). A `no_submit` Attempt retries at the next valid effort inside its letter (contract §9 'Next rung'), never landing on `X` when the model has efforts ≥ 1 (air-d16, air-d17, air-d21); a `crashed` or `merge_conflict` Attempt retries at the same Tier; after the top rung a `no_submit` fails the Job.
- **Worker.** An explore Worker may `cpd ask`: answer it as any Escalation. It may read the Context's records, read-only.
- **Refusals.** `explore_workflow_forbidden` (`--workflow`) and `explore_no_draft` (`--draft`): both exit 3. `illegal_phase` is not raised for an explore Job in any phase, `done` included: from `dispatch`, `deploy` or `done` the add moves the Plan back to `plan`. `cpd plan phase plan` is refused `explore_pending` (exit 3) while an explore Job of the Plan is queued, running or failed; `details.jobs` lists them.
- **A Manager's explore.** A Manager explores before a Lead exists through its own Lead-less shaping Plan: its explore Spec is the same, and its artefact is at `run/<plan>/explore/<key>.md` of that Plan.
