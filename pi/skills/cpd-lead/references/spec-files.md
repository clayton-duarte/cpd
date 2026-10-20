# Spec files, sizing, Tiers and Dependencies

A Worker is a fresh session that sees only its Spec. Whatever it needs must be in the spec file.

## The spec file

The file passed as `--spec` when you add a Job is JSON with exactly these fields, all required, and no others (an unknown field is refused with `spec_invalid`):

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
| `context` | Everything the Worker needs and cannot ask the User for: facts, paths, names, signatures, how to run things, decisions from the interview and the Context's records, the accepted assumptions that apply. |
| `touches` | Exact paths the Worker may change, at least one. New files included. The Worker may touch nothing else. |
| `acceptance` | At least one item, each checkable by a command or an observable result. Test items name the targeted tests: the test files or modules of what the Job changes plus its new tests (for example `scripts/cargo.sh nextest run -p cpd --test plan_overlap`, `node --test pi/test/updates.test.ts`). Never a full unfiltered run (the whole workspace, `npm test` of everything): the full suite is the Gate's, at `pr open`. |
| `out_of_scope` | The adjacent temptations to refuse. May be empty, rarely should be. |

Write the text in GFM only: tables, lists, code and Mermaid. Footnotes render as literal text and raw HTML is not rendered, so the User would see them broken in the Plan.

The Worker's submit runs only the repo's quick check (its `pre-commit` hook). The Gate (its `pre-merge` hook) runs later, on the whole Lead Branch, so an acceptance item that needs the Gate must name the command itself.

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

- Add `--needs <k1,k2>` when a Job changes files another Job changes, imports what another Job produces, or tests another Job's behaviour. A Job starts only after its Dependencies are Merged and branches from the Lead Branch that contains them.
- Jobs with disjoint `touches` and no data flow between them need no Dependency. Shared files such as lockfiles, `package.json`, config and index or route registries count as shared.
- Dependencies cannot form a cycle (`dependency_cycle`), cannot name an unknown Job (`needs_unknown`) and cannot name a cancelled or failed Job (`needs_unavailable`).

## Tiers

Format `<model><effort>`: models `O` (opus), `S` (sonnet), `H` (haiku); effort 0–5. An invalid Tier is refused when you add the Job (`tier_invalid`, with the valid efforts in `error.details.efforts`). `cpd tiers` prints the efforts each model accepts.

| Work | Tier | Why |
|---|---|---|
| Coding, tests, refactors | `S1` | The User's default. Start low: each failed Attempt that the ladder retries (`crashed`, `no_submit`, `merge_conflict`) raises effort by one on the next Attempt, up to `retry_cap` retries (default 2, so 3 Attempts). |
| Docs, config, mechanical edits | `H2` | Cheap and enough. |
| Judgement-heavy work (ambiguous design, security-sensitive, subtle algorithms) | `O3` | `O` only when judgement decides the outcome. |

## Research Jobs

Grounding that needs more than reading (a large codebase survey, comparing libraries, checking an API against its source) goes to a Worker as an ordinary Job.

- Spec: the question as `goal`; where to look and what a good answer contains as `context`; `touches` is the one Markdown file for the findings (for example `docs/research/<topic>.md`); `acceptance` says what the file must answer and cite. Tier `S1` to `S3`.
- Jobs run only after the lock, so a research Job cannot ground the Plan that contains it. Put it in the Plan first and add the follow-up Jobs as drafts right after the lock (`job add --draft`), listing them in its `context`. When it is Merged (`job-merged`), revise the drafts with what it found, then start them with `cpd_plan_review` (with a `title`). Tell the User what the batch holds.
- A research Job obeys the [bounded commands](#bounded-commands): it commits its findings file early and measures only under `timeout`, with real data, never a synthetic benchmark.
- If the question is cheap to answer by reading, read instead.
