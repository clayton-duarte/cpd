# t3: worktrunk recipes for Attempt worktrees, setup and rebase merges

Tier: S3 · Report: `~/projects/cpd/docs/trials/t3-worktrees-merge.md` · Read `_common.md` first.

## Goal

Produce tested, copy-paste-ready worktrunk (`wt` 0.80.0) recipes that `cpd` will run non-interactively for the whole Attempt worktree lifecycle:
- create from the Lead Branch HEAD;
- blocking setup;
- register in herdr;
- rebase + fast-forward merge keeping commits;
- the failure modes;
- cleanup;
- the final PR rebase.

## CPD rules this must satisfy

- Each Attempt gets its own worktree and branch created from the **current Lead Branch HEAD** (so it contains its Dependencies' commits). Setup (env files, dependency install) must **finish before** the Worker starts.
- An approved Attempt's commits are **kept (no squash)**, **rebased onto the Lead Branch and fast-forwarded** into it, one merge at a time. Linear history, trunk-based.
- The repo's **Gate** (a `verify` script: tests, typecheck, lint) must pass before the Lead Branch moves, and is run again on the Lead Branch after the merge.
- The Lead never edits code. The Lead Branch checkout must never be left dirty or half-merged.
- The PR is the Lead Branch against `main`. It is rebased onto `main` only when conflicts require it.

## Constraints

- Worktrunk keeps user config and hook approvals under the user's config dir (`~/.config/worktrunk/…`). Run **every** `wt` command with `HOME=/tmp/cpd-trials/t3/home` and `XDG_CONFIG_HOME=/tmp/cpd-trials/t3/home/.config` so nothing reaches the real home. List what wt wrote there at the end.
- Docs: `wt --help`, `wt <cmd> --help`, and the source at `/tmp/cpd-research/repos/max-sixty__worktrunk` (`docs/public/merge.md`, `hook.md`, `switch.md`, `step.md`; `src/commands/worktree/push.rs` around lines 160–400).
- herdr: only `herdr --session cpd-trial …`; create a workspace labelled `t3`.

## Setup

1. Scratch repo `/tmp/cpd-trials/t3/repo`:
   - `main` with 3 commits;
   - a `verify.sh` that exits 1 if a file `FAIL_GATE` exists in the worktree, else 0;
   - `.config/wt.toml` with a `pre-start` hook (sleeps 5 s, writes `.setup-done` with a timestamp, copies a gitignored `.env` from the main checkout) and a `pre-merge` hook running `./verify.sh`.
2. Lead Branch `cpd/demo` created from `main`. Decide and justify where it is checked out (its own wt worktree vs the main checkout) given "never dirty, never half-merged".

## Acceptance criteria

1. **must** Create: an Attempt worktree `cpd/demo/j1-a1` from the Lead Branch HEAD.
   - Prove the base: `git merge-base` equals the Lead Branch HEAD SHA at creation.
   - Prove `pre-start` **blocked** until `.setup-done` existed: timestamps vs command return.
   - Find the non-interactive invocation: no TTY prompts, hook approval via `--yes` or equivalent, no shell `cd` integration needed, machine-readable output if available. Show the exit codes.
2. **must** herdr registration: `herdr --session cpd-trial worktree open …` makes it a grouped workspace; show `worktree list` and `workspace list` output.
3. **must** Rebase + fast-forward merge, commits kept:
   - Make 2 commits in `j1-a1`.
   - Meanwhile advance the Lead Branch with another Attempt `j2-a1` merged first.
   - Merge `j1-a1` with `wt merge` (`--no-squash`, keep the worktree with `--no-remove` unless you justify otherwise).
   - Show `git log --graph --oneline` of the Lead Branch: linear, both commits present with original messages and authors, new SHAs after rebase.
   - Show the Lead Branch checkout updated and clean.
4. **must** Gate failure: with `FAIL_GATE` present, the merge aborts and the Lead Branch ref and checkout are unchanged. Show the exit code and message.
5. **must** Rebase conflict: two Attempts editing the same line. Show what state the Attempt worktree is left in, how to detect it from exit code or output, and the clean abort recipe that leaves the Lead Branch untouched.
6. **must** Concurrent merges: start two `wt merge` commands into the Lead Branch at the same moment (`&`). Show that exactly one wins and the other fails cleanly (compare-and-swap), and that rerunning it succeeds.
7. **should** Dirty Lead Branch checkout: an uncommitted change in the Lead Branch worktree during a merge. Show the behaviour.
8. **should** Cleanup semantics: remove a merged Attempt (worktree and branch) and keep a failed Attempt's worktree. Show the herdr side (workspace closed or not) and `git worktree list`.
9. **should** PR rebase: advance `main` by 2 commits (one conflicting with the Lead Branch). Show the recipe to rebase the Lead Branch onto `main` and how a conflict is surfaced.
10. **should** Gate after merge: run `verify.sh` on the Lead Branch after each merge; recipe and cost.

## Deliverable extras

In **Recipes**, give the exact command sequences `cpd` should run for:
- `attempt create`;
- `attempt ready` (setup done);
- `attempt merge`, with exit-code handling for success, gate-failed, conflict, cas-lost and dirty-target;
- `attempt remove`;
- `pr rebase`.

Also list the minimal per-repo `.config/wt.toml` a CPD-enabled repo needs.
