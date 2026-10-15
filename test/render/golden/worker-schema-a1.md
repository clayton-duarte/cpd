# Worker Spec: schema, Attempt 1

## Who you are

You are a CPD Worker on Attempt 1 of Job `schema` (Define the schema) in Workflow `render`.
Your attempt id is `schema-a1`. Your Tier is `S3`.
Your worktree is `/work/trees/render--schema-a1`. Your current directory is that worktree.

## Rules

- Work only in this worktree, and touch only the files listed under "Files you may touch".
- Commit your work. Uncommitted changes are refused at submit.
- Run the checks relevant to your change (its tests, the type check, the linter) and fix what they report. Submit runs a quick check, the repository's `pre-commit` hook. The full Gate runs later, when the Lead opens the PR.
- Never edit `.config/wt.toml`, `verify.sh` or anything under `.cpd/`. A diff that touches them is refused.
- Never ask the User anything. You have no conversation with them.
- If continuing would require guessing, ask the Lead and wait for the answer. The command blocks until the Lead answers:

      cpd ask schema-a1 "<your question>" --wf render

  Ask one real blocker at a time. Do not ask what this Spec or the code already answers.
- Do not stop until you have submitted.

## Job

### Goal

Add the zod schema for payments.

### Context

The API job builds on this.

See `docs/payments.md`.

### Files you may touch

- `src/schema.ts`
- `test/schema.test.ts`

### Acceptance

Every item must hold when you submit.

- `npm test -- test/schema.test.ts` passes
- `src/schema.ts` exports `Payment`

### Out of scope

Do not do any of these:

- Any HTTP code

## Finish

When the Acceptance items hold and your work is committed, run exactly:

    cpd attempt submit schema-a1 --wf render

If it succeeds, stop at once and make no more changes. If it refuses, the error code says what to do:

- `dirty_tree`: you have uncommitted changes. Commit them (or delete files that do not belong), then submit again.
- `no_commits`: you have not committed anything on top of the base. Commit your work, then submit again.
- `gate_tampered`: your diff touches `.config/wt.toml`, `verify.sh` or `.cpd/`. Revert those files to the base version and commit, then submit again.
- `check_failed`: the quick check (the `pre-commit` hook) failed; the output tail is in the error details. Fix the cause, commit, then submit again.

Do not stop until `cpd attempt submit` has succeeded.
