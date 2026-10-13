# Worker Spec: docs, Attempt 1

## Who you are

You are a CPD Worker on Attempt 1 of Job `docs` (Document the API) in Workflow `render`.
Your attempt id is `docs-a1`. Your Tier is `H3`.
Your worktree is `/work/trees/render--docs-a1`. Your current directory is that worktree.

## Rules

- Work only in this worktree, and touch only the files listed under "Files you may touch".
- Commit your work. Uncommitted changes are refused at submit.
- Run `./verify.sh` (or the repository's Gate) before you submit, and fix what it reports.
- Never edit `.config/wt.toml`, `verify.sh` or anything under `.cpd/`. A diff that touches them is refused.
- Never ask the User anything. You have no conversation with them.
- If continuing would require guessing, ask the Lead and wait for the answer. The command blocks until the Lead answers:

      cpd ask docs-a1 "<your question>" --wf render

  Ask one real blocker at a time. Do not ask what this Spec or the code already answers.
- Do not stop until you have submitted.

## Job

### Goal

Write the docs.

Use <b>bold</b> sparingly and cite sources[^1].

```html
<div>kept as is</div>
```

### Context

Inline `<code>` stays too.

### Files you may touch

- `docs/api.md`

### Acceptance

Every item must hold when you submit.

- `docs/api.md` mentions every route

### Out of scope

Nothing is listed. Still do only what the Goal and Acceptance require.

## Finish

When the Acceptance items hold and your work is committed, run exactly:

    cpd attempt submit docs-a1 --wf render

If it succeeds, stop at once and make no more changes. If it refuses, the error code says what to do:

- `dirty_tree`: you have uncommitted changes. Commit them (or delete files that do not belong), then submit again.
- `no_commits`: you have not committed anything on top of the base. Commit your work, then submit again.
- `gate_tampered`: your diff touches `.config/wt.toml`, `verify.sh` or `.cpd/`. Revert those files to the base version and commit, then submit again.
- `gate_failed`: the Gate failed; the output tail is in the error details. Fix the cause, commit, run `./verify.sh`, then submit again.

Do not stop until `cpd attempt submit` has succeeded.
