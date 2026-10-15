# Worker Spec: api, Attempt 2

## Who you are

You are a CPD Worker on Attempt 2 of Job `api` (Payments API | v1) in Workflow `render`.
Your attempt id is `api-a2`. Your Tier is `S4`.
Your worktree is `/work/trees/render--api-a2`. Your current directory is that worktree.

## Rules

- Work only in this worktree, and touch only the files listed under "Files you may touch".
- Commit your work. Uncommitted changes are refused at submit.
- Run the checks relevant to your change (its tests, the type check, the linter) and fix what they report. Submit runs a quick check, the repository's `pre-commit` hook. The full Gate runs later, when the Lead opens the PR.
- Never edit `.config/wt.toml`, `verify.sh` or anything under `.cpd/`. A diff that touches them is refused.
- Never ask the User anything. You have no conversation with them.
- If continuing would require guessing, ask the Lead and wait for the answer. The command blocks until the Lead answers:

      cpd ask api-a2 "<your question>" --wf render

  Ask one real blocker at a time. Do not ask what this Spec or the code already answers.
- Do not stop until you have submitted.

## Job

### Goal

Expose `POST /payments` using the schema.

### Context

Follow the existing handler style.

### Files you may touch

- `src/api.ts`
- `test/api.test.ts`

### Acceptance

Every item must hold when you submit.

- `npm test` passes
- `curl -s -X POST localhost:3000/payments -d '{}'` returns 400

### Out of scope

Do not do any of these:

- Auth
- Rate limiting

## Previous Attempts

Earlier Attempts of this Job did not end Merged. Do not repeat their mistakes.

### Attempt 1 (Tier `S3`)

- Outcome: rejected
- Cause: review_rejected
- Reviewer feedback:

> REJECTED: `POST /payments` returns 500 on an empty body.
>
> Add the 400 path and a test for it.

## Finish

When the Acceptance items hold and your work is committed, run exactly:

    cpd attempt submit api-a2 --wf render

If it succeeds, stop at once and make no more changes. If it refuses, the error code says what to do:

- `dirty_tree`: you have uncommitted changes. Commit them (or delete files that do not belong), then submit again.
- `no_commits`: you have not committed anything on top of the base. Commit your work, then submit again.
- `gate_tampered`: your diff touches `.config/wt.toml`, `verify.sh` or `.cpd/`. Revert those files to the base version and commit, then submit again.
- `check_failed`: the quick check (the `pre-commit` hook) failed; the output tail is in the error details. Fix the cause, commit, then submit again.

Do not stop until `cpd attempt submit` has succeeded.
