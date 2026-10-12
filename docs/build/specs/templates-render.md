# Job `templates-render`: Worker Spec, review brief and plan rendering

Tier S3 · Needs: `ledger-core` · Read `_common.md` first. Contract sections: §11 (`plan render`), §12 steps 3 and 9, §16 (templates). Requirements: `docs/requirements.md` §2 (Plan, Dispatch).

## Goal
Turn Ledger data into the three documents agents and the User read:
- the Worker Spec for one Attempt;
- the Reviewer's brief;
- the plan Markdown that plannotator shows the User.

## Files you own
`templates/worker.md`, `templates/reviewer.md`, `src/render/template.ts`, `src/render/spec.ts`, `src/render/review.ts`, `src/render/plan.ts`, `src/commands/plan-render.ts`, `test/render/**`, `test/commands/plan-render.test.ts`.

## Requirements
- `template.ts`: minimal `{{name}}` substitution plus `{{#each list}}…{{/each}}` and `{{#if x}}…{{/if}}`, nothing more. An unknown placeholder or a missing value is an error (fail loud).
- `templates/worker.md` (rendered to `specs/<attempt-id>.md`):
  - **Who you are:** a CPD Worker on Attempt n of Job k; your Tier; your worktree path.
  - **Rules:**
    - work only in this worktree and touch only the listed files;
    - commit your work;
    - run `./verify.sh` (or the repo's Gate) before submitting;
    - never edit `.config/wt.toml`, `verify.sh` or `.cpd/`;
    - never ask the User; if continuing would require guessing, ask the Lead with the exact `cpd ask` command;
    - do not stop until submitted.
  - **Job:** goal, context, files you may touch, acceptance (each a command or check), out of scope.
  - **Previous Attempts** (n > 1): outcome, cause and Reviewer feedback of each.
  - **Finish:** the exact `cpd attempt submit <id> --wf <wf>` command and what to do on each refusal code (`dirty_tree`, `no_commits`, `gate_tampered`, `gate_failed`).
- `templates/reviewer.md` (rendered to `reviews/<attempt-id>.md`):
  - you are a read-only Reviewer and cannot change files;
  - the inputs: the Spec as given to the Worker, the diff file path, the Gate output file path, and the worktree path to read full files;
  - the rubric: approve only if every acceptance item is met; no changes outside the Spec; touched files fit "files you may touch"; no obvious defects or missing tests the acceptance implies;
  - end with exactly one fenced `json` block `{"verdict": "approve" | "reject", "feedback": "<specific, actionable>"}` as the last thing in the message.
- `spec.ts`: `renderWorkerSpec(wf, attemptId)` reads the Job, Attempts and feedback from the Ledger (via `ledger-core` and, if present in your base, `ledger-attempts`) and writes the file, returning its path.
- `review.ts`: `renderReviewBrief(wf, attemptId, {diffPath, gatePath, worktree})`.
- `plan.ts` + `cpd plan render [--out <file>]`, per §11. Default output `<wf>/plan.md`. Returns `{ok, path, fingerprint}`, using `planFingerprint` from `ledger-core`.

## Acceptance
1. Golden-file tests: a Ledger fixture (real `bd`, via the ledger-core API) with 3 Jobs and one Job on Attempt 2 with feedback. The rendered Worker Spec, review brief and plan match committed golden files under `test/render/golden/`. Update the goldens deliberately; never auto-update in tests.
2. The plan Markdown contains a GFM table, a Mermaid `graph TD` with one edge per Dependency, and one section per Job. It contains no footnotes or raw HTML (asserted).
3. A missing template value throws a named error.
4. The rendered Worker Spec for Attempt 1 contains the exact `cpd attempt submit` and `cpd ask` command lines with the right ids and `--wf`.

## Out of scope
The Lead skill; deciding when to render (dispatch Jobs).
