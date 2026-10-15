# Spec files, sizing, Tiers and Dependencies

A Worker is a fresh session that sees only its Spec. Whatever it needs must be in the spec file.

## The spec file

The file passed as `--spec` when you add a Job is JSON with exactly these fields, all required:

```json
{
  "goal": "Add a /health endpoint that returns 200 and {\"status\":\"ok\"}.",
  "context": "The server is src/server.ts (Express 5). Routes are registered in src/routes/index.ts. Accepted assumption A2: no auth on /health.",
  "touches": ["src/routes/health.ts", "src/routes/index.ts", "test/routes/health.test.ts"],
  "acceptance": [
    "GET /health returns 200 with body {\"status\":\"ok\"}",
    "`npm test -- test/routes/health.test.ts` passes",
    "`./verify.sh` passes"
  ],
  "outOfScope": ["Do not change the existing routes", "Do not add a dependency"]
}
```

| Field | What it holds |
|---|---|
| `goal` | One outcome, in the imperative. |
| `context` | Everything the Worker needs and cannot ask the User for: facts, paths, names, signatures, how to run things, decisions from the interview, the accepted assumptions that apply. |
| `touches` | Exact paths the Worker may change, at least one. New files included. The Worker may touch nothing else. |
| `acceptance` | At least one item, each checkable by a command or an observable result. |
| `outOfScope` | The adjacent temptations to refuse. May be empty, rarely should be. |

Write the text in GFM only: tables, lists, code and Mermaid. Footnotes render as literal text and raw HTML is not rendered, so the User would see them broken in the Plan.

## Size

- Target **about 13 min and about 150 turns** per Job, including worktree setup and dependency install. The timeout is **30 min** per Attempt. A Job that needs more fails and costs a retry.
- A Job is one outcome that one Worker can finish and verify. Two independent outcomes are two Jobs.
- Prefer many small Jobs that run in parallel. Do not split below a unit that has its own acceptance check.
- A Job keyed `^[a-z][a-z0-9-]{0,31}$` and unique in the Workflow.

## Dependencies

- Add `--needs <k1,k2>` when a Job changes files another Job changes, imports what another Job produces, or tests another Job's behaviour. A Job starts only after its Dependencies are Merged and branches from the Lead Branch that contains them.
- Jobs with disjoint `touches` and no data flow between them need no Dependency. Shared files such as lockfiles, `package.json`, config and index or route registries count as shared.
- Dependencies cannot form a cycle, and cannot name a cancelled or failed Job.

## Tiers

Format `<model><effort>`: models `O` (opus), `S` (sonnet), `H` (haiku); effort 0–5. Valid: `O1`–`O5`, `S0`–`S5`, `H0`–`H3`. An invalid Tier is refused when you add the Job. `cpd tiers` prints the efforts each model accepts.

| Work | Tier | Why |
|---|---|---|
| Coding, tests, refactors | `S1` | The User's default. Start low: each rejection raises effort by one on the next Attempt, up to 2 retries (3 Attempts). |
| Review (`--review-tier`) | `S4` | The default review Tier. Raise it for risky changes. Unused when `reviewMode` is `none` (no Reviewer). |
| Docs, config, mechanical edits | `H2` | Cheap and enough. |
| Judgement-heavy work (ambiguous design, security-sensitive, subtle algorithms) | `O3` | `O` only when judgement decides the outcome. |

## Research Jobs

Grounding that needs more than reading (a large codebase survey, comparing libraries, checking an API against its source) goes to a Worker as an ordinary Job.

- Spec: the question as `goal`; where to look and what a good answer contains as `context`; `touches` is the one Markdown file for the findings (for example `docs/research/<topic>.md`); `acceptance` says what the file must answer and cite. Tier `S1` to `S3`.
- Jobs run only after the lock, so a research Job cannot ground the Plan that contains it. Put it in the Plan first, list in its `context` the follow-up Jobs you will append, and append them when it is Merged (`job-merged`). Tell the User what you appended.
- If the question is cheap to answer by reading, read instead.
