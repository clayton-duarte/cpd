# The refusal miner (`pi/tools/refusal-miner`)

A read-only TypeScript tool that mines **guard refusals** out of pi's session files and prints the
ranking tables: refusals by guard and error code, broken down by agent role (Manager, Lead, Worker,
scout) and by model, each row quoting the exact matched string. The library is
`pi/lib/refusal-miner.ts` (pure: session file contents plus a since timestamp in, tables out); the
CLI entry is `pi/tools/refusal-miner.ts`, run by the package.json script `refusals`.

**The miner is the historical backfill behind the guard Event.** Going forward the guard Event
(contract §5.2 — every guard refusal is appended as a guard Event) is the source of the refusal
record; the miner covers only the history the Events do not reach: the sessions written before the
Event existed. It reads session files; it reads no store, runs no command and writes no file.

Scout artefacts this contract is built from (read-only evidence, `$CPD_HOME` =
`/Users/claytonduarte/.cpd`): `run/agent-refusals/explore/scout-guards.md` (the refusal signatures
and the session-file format), `scout-ranking.md` (the hand-done ranking this tool mechanises),
`scout-tests.md` and `scout-test-names.md` (where the tests live and how a pi test is named).

## Inputs

- **Every session directory under `~/.pi/agent/sessions/`** — all keys, not only
  `--Users-claytonduarte--`: the home key holds the Manager's, Leads' and plain user sessions; the
  run-directory keys (`--…-.cpd-run-…--`) hold Workers' and scouts' sessions. Skipping the
  run-directory keys hides every Worker and scout refusal (scout-ranking §2: their counts are 0
  there by scope, not by behaviour).
- **Every `*.jsonl` file in each directory**, one JSON object per line, all lines the same object
  shape (`type`, `id`, `parentId`, top-level `timestamp`, `message`; scout-guards §3).
- Read-only: no file under the sessions root is ever written or changed.

## The record filter

A record counts when its **top-level `timestamp`** (ISO-8601, UTC `Z`) is at or after the
`--since` cutoff (§5.7 of the contract; the same filter `pi/measure/session-bloat.ts` applies).
The `message`-inner millisecond epoch timestamp is a different field and is not the filter.

## What counts as a refusal

Two families (scout-guards §2 and §3):

1. **Adapter guard refusal** — a `message` with `role: "toolResult"`, first content `text` starting
   with `φ: ` (U+03C6 + a space), `isError: true`, `details: {}`. The call never ran: no command
   output, no exit code. The guard is identified by the exact prefix of the text (table below).
2. **CLI refusal** — a `bash` or `codemode` toolResult whose output carries a serialized refusal
   envelope `{"ok":false,"error":{"code":"<code>",…}}` (core/src/error.rs; exit 2 usage, 3
   refused). Counted once per envelope occurrence; the code is the envelope's `"code"` field.
   A refusal that is only **quoted** inside another output is not counted: an envelope not at the
   start of a line (count-prefixed output, `file:line:` grep prefixes), and the whole output of a
   pure evidence reader (`cpd events list`, digests, session files read back).

## The output tables

Three tables, printed in this order, plus totals (scout-ranking §1–§3 is the worked example):

1. **Refusals by guard / error code**, highest count first. Adapter rows are named by guard
   (`φ: <guard>`); CLI rows are `cli: <code>`. Every row **quotes the exact matched string** —
   the refusal prefix or the envelope shape it matched on.
2. **The same breakdown by role**: Manager, Lead, Worker, scout — one column each.
3. **The same breakdown by model** (`provider/modelId`).

**Role attribution.** A session's role is read from the session itself: the loaded cpd skill names
Manager and Lead (`cpd-manager` → Manager, `cpd-lead` → Lead); a session under a run-directory key
is a Worker or a scout — a scout's cwd is the explore directory
(`$CPD_HOME/run/<plan>/explore/`, contract §3.4), so run-directory keys under an `explore/`
directory are scouts and every other run-directory key is a Worker.

**Model attribution.** The refusal's model is the session's last `model_change` record
(`provider/modelId`) at or before the refusal's timestamp.

**Guard identity — the exact matched strings** (scout-guards §2 is the full table; the set of CLI
codes is open, whatever the envelope's `code` field says — `usage`, `spec_invalid`, `lint_failed`,
`outline_invalid`, `shape_incomplete`, `touches_missing`, `shape_violation`,
`illegal_transition`, … are the observed heavy ones):

| Guard | Exact matched string (prefix) |
|---|---|
| loop_guard_read_streak (>12) | `φ: loop guard: this tool call was NOT executed: more than 12 consecutive read-only tool calls in one turn.` |
| loop_guard_read_streak (>30) | `φ: loop guard: this tool call was NOT executed: more than 30 read-only tool calls in one turn.` |
| noblock_lead | `φ: a Lead never blocks: a wait over 30 s runs in the background or is left to the engine's Events; refused: bash timeout <N>` |
| noblock_manager | `φ: a Manager never blocks: a wait over 30 s runs in the background or is the owning Lead's or Worker's job; refused: bash timeout <N>` |
| explore_bash_timeout | `φ: explore timeout: every bash call carries a timeout above 0 and at most 30 s, except a lone cpd ask, which has no upper bound; got <none\|N>` |
| explore_write | `φ: explore write: write only under <CPD_HOME>/run/<plan>/explore/ or the temp dir: <path>` |
| explore_network / explore_network_write | `φ: explore network write: scout has no network: git fetch` (plain commands: `φ: explore network: scout has no network: <command>`) |
| primary_checkout | `φ: no agent moves HEAD or changes the files of a repo's primary checkout; fetching is allowed, …; refused: <edit\|write\|bash> <path> …` |
| ask_user_question_guard | `φ: ask_user_question is for a few short choices; write anything larger to a file outside the repo and point to it in chat; refused: <N> characters (limit 1500)` and `φ: state what you ask about in the question itself (its text, or an option's description or preview); refused: …` |
| explore_artefact_read | `φ: explore artefact: $CPD_HOME/run/<plan>/explore is for Workers; read the job-merged summary or add a follow-up scout Job` |
| cli: `<code>` | `{"ok":false,"error":{"code":"<code>",…}}` |
| cli: (no code field) | the verbatim envelope shape, e.g. `{"ok":false,"id":null}` |

Other guards wired in the same `tool_call` handler share the same shape and are matched by their own
prefix (the full-test-run, raw-op, electron and host-approve guards; scout-guards §2).

## Example call per role

One call shape: from the repo root, `pnpm run refusals -- --since <ISO timestamp>`. An agent role
wraps it in `timeout` (a Worker rule); a scout redirects the tables into its explore artefact.

- **Manager** — the whole machine, since the Plan opened, to brief the User:
  ```sh
  pnpm run refusals -- --since 2026-11-07T00:00:00.000Z
  ```
- **Lead** — its Plan's window, to see what its Workers and its Specs tripped:
  ```sh
  pnpm run refusals -- --since 2026-11-07T16:00:00.000Z
  ```
- **Worker** — wrapped in `timeout`, like every command it runs:
  ```sh
  timeout 300 pnpm run refusals -- --since 2026-11-07T00:00:00.000Z
  ```
- **scout** — the ranking written to its artefact under the explore directory:
  ```sh
  timeout 300 pnpm run refusals -- --since 2026-11-07T00:00:00.000Z \
    > "$CPD_HOME/run/<plan>/explore/refusal-ranking.md"
  ```

**An example refused call per role** — what the miner finds and which row it lands in:

| Role | Refused call | Exact matched string | Table row |
|---|---|---|---|
| Manager | `bash` with `timeout 60` | `φ: a Manager never blocks: a wait over 30 s runs in the background or is the owning Lead's or Worker's job; refused: bash timeout 60` | `φ: noblock_manager`, role Manager |
| Lead | `cpd job add` with a bad Spec | `{"ok":false,"error":{"code":"spec_invalid","message":"The Spec is invalid at $.touches: …"}}` | `cli: spec_invalid`, role Lead |
| Worker | the 13th consecutive read-only tool call | `φ: loop guard: this tool call was NOT executed: more than 12 consecutive read-only tool calls in one turn.` | `φ: loop_guard_read_streak (>12)`, role Worker |
| scout | `bash` running `git fetch` | `φ: explore network write: scout has no network: git fetch` | `φ: explore_network_write`, role scout |

## The tests

The miner's tests are `pi/test/refusal-miner.test.ts` (pi test id form
`pi/test/<file>.test.ts::<exact title>`, scout-test-names §1) over fixtures under
`pi/test/refusal-miner/fixtures/` — small synthetic session files, **never** a real session file of
the User or of any live agent, and never the full suite (the Gate runs it). Run one file:
`timeout 300 pnpm test pi/test/refusal-miner.test.ts`.
