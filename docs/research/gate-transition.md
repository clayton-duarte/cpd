# Gate transition: failed Actions checks and `cpd/gate` (hs-d282)

Job `gate-transition`, attempt 1, 2026-10-19. Repo `clayton-duarte/cpd` (private, free plan).

## Answer

1. **Deleting a workflow run removes its check runs at once.** `gh run delete <id>` took the `rust` and `pi` check runs off the commit within one poll (under 15 s). The commit then shows only GitGuardian (success). No new commit is needed to clear the failed checks.
2. **A `cpd/gate` success status is accepted on any commit, with or without failed check runs, and is a first-class entry in the PR's `statusCheckRollup`.** It does not hide failed check runs: with the failures still present the commit has `pi=failure, rust=failure, cpd/gate=success` side by side (the status never overrides a check run). So a PR with a failed run on its head is not CLEAN with `cpd/gate` alone; delete the run too. After the deletion only `cpd/gate=success` and GitGuardian remain.
3. **A new commit is needed only for a different reason: a content conflict with `main`.** `mergeStateStatus` is `DIRTY` for a conflicting PR whatever its checks say. #109 is `DIRTY`/`CONFLICTING` with `cpd/gate=success` and GitGuardian success on its head. Its conflict is real (`git merge-tree --write-tree origin/main <head>`: conflicts in `core/src/pr/mod.rs` and `docs/contract.md`). Only a rebase or a merge of `main` clears it, i.e. a new commit, i.e. `cpd pr rebase` or a merge commit pushed by `cpd pr push`.

What I could not reproduce (unverified): a PR that is `UNSTABLE` only because of a failed run on its *current head*, going to `CLEAN` after deletion. Neither #100 (merged) nor #109 (conflicting) has such a head, and no new head can get a run now: the `CI` workflow is `disabled_manually` (updated 2026-10-19T09:55:55-04:00), so `gh workflow run ci.yml` answers 422 and a new push starts no run. I did not re-enable it (a repo setting; out of scope). I tested the same mechanism on older commits of both PRs (the runs' `head_sha`s). A PR's rollup is computed from its head commit's check runs and statuses (shown on #109's head), so the commit-level result carries over. I infer that, from the head's check-runs endpoint changing; the `UNSTABLE` to `CLEAN` step itself is unobserved.

State of the 13 open PRs in cpd (read-only list, `gh pr list --state open --json number,mergeStateStatus`): `DIRTY` #111, #109, #108, #107, #106, #104, #96, #95, #93, #80 (all conflict with `main`, so they need a new commit); `UNSTABLE` #103, #98, #97 (not conflicting; unverified that the cause is a failed run on the head: each has the pre-billing failed `CI` run in `gh run list`, which I did not inspect further, other PRs being out of scope). No branch protection exists (403 "Upgrade to GitHub Pro"), so no check is *required*; `UNSTABLE` is GitHub's non-required-failure state, and `CLEAN` needs none failing or pending.

## Commands and output

Baseline (read-only):

```
$ gh pr view 109 --json headRefName,headRefOid,mergeStateStatus,mergeable,state
cpd/cross-repo/nolock dfcf6f43c308eb271b5a510163ef7867676daa73 DIRTY CONFLICTING OPEN
$ gh pr view 100 --json headRefName,headRefOid,state
cpd/cross-repo/distill2 134f4de7de338dc2655f16fa150a6a4b2cea77ad MERGED
$ gh api repos/clayton-duarte/cpd/commits/<head>/check-runs      # both heads
GitGuardian Security Checks completed success                    # only this; no Actions run on either head
$ gh run list --branch cpd/cross-repo/nolock --json databaseId,headSha,conclusion,event
[{"conclusion":"failure","databaseId":39220427306,"event":"pull_request","headSha":"f48016eb1c95…"}]   # an older commit of #109
$ gh run list --branch cpd/cross-repo/distill2 …
[{"conclusion":"failure","databaseId":39210483841,"event":"pull_request","headSha":"13c98e012532…"}]   # an older commit of #100
$ gh api repos/clayton-duarte/cpd/commits/f48016eb1c95…/check-runs
3: GitGuardian success, pi failure, rust failure
$ gh workflow list --all
CI  disabled_manually  386987857
$ gh workflow run ci.yml --ref cpd/cross-repo/nolock
could not create workflow dispatch event: HTTP 422: Cannot trigger a 'workflow_dispatch' on a disabled workflow
```

Why the heads have no runs: #109's head is a merge commit made at 14:09Z, #100's head came after its run; the workflow was disabled at 13:55Z.

Experiment 1: delete the failed run of a #109 commit (`f48016eb1c95`, once its head):

```
$ gh run delete 39220427306
✓ Request to delete workflow submitted.
# polls at 15, 30, 45, 60 s:
$ gh run list --branch cpd/cross-repo/nolock --json databaseId,headSha,conclusion
[]
$ gh api repos/clayton-duarte/cpd/commits/f48016eb1c95…/check-runs
1: GitGuardian Security Checks completed success         # pi and rust are gone from the first poll
$ gh api repos/clayton-duarte/cpd/commits/f48016eb1c95…/status
pending 0                                                # no statuses; "pending" is the combined state of an empty set
```

Experiment 2a: `cpd/gate` on #109's head, DIRTY PR:

```
$ gh api -X POST repos/clayton-duarte/cpd/statuses/dfcf6f43c308… -f state=success -f context=cpd/gate -f description='cpd Gate (manual test)'
cpd/gate success cpd Gate (manual test)
$ gh pr view 109 --json mergeStateStatus,mergeable,statusCheckRollup   # 3 polls, 10 s apart
DIRTY CONFLICTING GitGuardian Security Checks=SUCCESS,cpd/gate=SUCCESS
$ gh api repos/clayton-duarte/cpd/commits/dfcf6f43c308…/status
{"state":"success","statuses":[["cpd/gate","success","cpd Gate (manual test)"]]}
$ git merge-tree --write-tree origin/main dfcf6f43c308…
CONFLICT (content): Merge conflict in core/src/pr/mod.rs
CONFLICT (content): Merge conflict in docs/contract.md
```

Experiment 2b: `cpd/gate` before and after the deletion, on a #100 commit that carries failed runs (`13c98e012532`):

```
$ gh api -X POST repos/clayton-duarte/cpd/statuses/13c98e012532… -f state=success -f context=cpd/gate -f description='cpd Gate (manual test)'
cpd/gate success
# before the deletion
check-runs: pi=failure,rust=failure,GitGuardian Security Checks=success
status:     {"state":"success","statuses":[["cpd/gate","success"]]}
$ gh run delete 39210483841
✓ Request to delete workflow submitted.
# 20 s later
check-runs: GitGuardian Security Checks=success
status:     {"state":"success","statuses":[["cpd/gate","success"]]}
```

Side effects left behind (a status cannot be deleted): `cpd/gate=success` "cpd Gate (manual test)" on `dfcf6f43c308` (#109 head) and on `13c98e012532` (#100's old commit). The former is a false claim for a commit that never passed the Gate here; the next real status on a new head replaces nothing but is on a different sha. Deleted runs: 39220427306, 39210483841 (failed, billing; no loss).

## Steps for each Lead, once PR gateci (the status) is installed

Preconditions (the User or a Lead, once): gateci merged; the repo's `.config/cpd.toml` has `[gate]` `status = true` (a commit of the User or a Lead, never a Job, §7.5); `ci.yml` is `workflow_dispatch` only on `main`. Order: merge gateci and the workflow change before anything re-enables the `CI` workflow. A branch that still carries the old `ci.yml` (with `pull_request`) would start a failing run on every push once the workflow is enabled again (unverified: GitHub reads a `pull_request` workflow from the merge ref). Leave `CI` disabled until the branches carry the new file, or accept that it only runs by hand.

For each open PR of the Plan (`<key>`, `<branch>`), in its Lead worktree:

1. Read: `gh pr view <n> --json mergeStateStatus,mergeable,statusCheckRollup`.
2. `DIRTY`/`CONFLICTING` (case B) go to "Case B". Anything else goes to "Case A".

**Case A: no new commit needed** (`UNSTABLE`, no conflict; the head has failed `CI` check runs):

1. Delete the failed runs of the head commit:
   `H=$(git rev-parse HEAD); gh run list --branch <branch> --limit 100 --json databaseId,headSha,conclusion --jq ".[]|select(.headSha==\"$H\" and .conclusion==\"failure\")|.databaseId" | xargs -r -n1 gh run delete`
   (Runs on older commits do not count towards the head and can stay.)
2. Get the status on the head through the Gate: `cpd pr push --pr <key>`. The branch is already at the remote, so it runs the Gate, pushes nothing and returns `"pushed": false` (commands.md §9.9). Whether the status is posted when nothing was pushed is **unverified** (the Lead's details say after "the push of the gated commit succeeded", rule P: the remote has the commit). Check: `gh api repos/<owner>/<repo>/commits/$H/status --jq '.statuses[]|[.context,.state]'`.
3. If it is absent and the Gate passed in step 2 (`ok: true`), post it by hand, naming the Gate: `gh api -X POST repos/<owner>/<repo>/statuses/$H -f state=success -f context=cpd/gate -f description='cpd Gate: npm run verify'`, and tell the User that `pr push` leaves no status when up to date (a defect in gateci).
4. Poll `gh pr view <n> --json mergeStateStatus,statusCheckRollup` up to a minute; expect `CLEAN` and `cpd/gate=SUCCESS`.

**Case B: a new commit is needed** (`DIRTY`, a content conflict with the base):

1. In the Lead worktree: `git fetch origin && git merge origin/<base>`. Resolve the conflicts (hand edits, hs-d269); commit the merge. Or, if the branch has no merge of the base to keep: `cpd pr rebase --pr <key>` (it rebases, runs the Gate and pushes with a lease; it posts the status itself, so skip to step 4).
2. `cpd pr push --pr <key>`: it runs the Gate on the merged head and pushes plain (never a hand `git push`, hs-d282 (4)); with `[gate] status = true` it posts `cpd/gate` on the pushed sha. `push_rejected` means the remote moved: `cpd pr rebase --pr <key>`.
3. Failed runs on the old head do not matter any more (the new head has none), so there is nothing to delete. Delete the old ones only for tidiness (step 1 of Case A with the old sha).
4. Poll `gh pr view <n> --json mergeStateStatus,statusCheckRollup`; expect `CLEAN`.

On any `gate_failed`: nothing was pushed and no status is posted; the Lead appends fix Jobs, as today.

The 10 `DIRTY` PRs above are Case B; the three `UNSTABLE` ones are Case A (if step 1 of Case A finds no failed run on the head, the cause is something else: read `statusCheckRollup`).

## What the User runs, if a step needs the account owner

- `gh run delete` and statuses need only `repo` scope (clayton-duarte's token has it; I ran both): no owner step.
- `gh workflow enable ci.yml` / `disable` (an Actions setting) is the owner's. Needed only if someone wants the `workflow_dispatch` runs back. Today the workflow is disabled, and the repo has no branch protection, so nothing else is blocked.
- If the Lead is not `clayton-duarte` or lacks `repo` scope on another repo, the `statuses` POST and the `run delete` return 403/404: the User runs the same commands under their own account (`gh auth status` first).
- Branch protection with a required `cpd/gate` check needs GitHub Pro or a public repo (403 here): the User's call, not needed for CLEAN.
