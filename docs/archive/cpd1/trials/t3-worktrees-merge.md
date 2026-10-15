# t3: worktrunk recipes for Attempt worktrees, setup and rebase merges

Date 2026-10-12 (system clock 18:27 → 18:31 EDT, about 5 min of wall time, plus report writing). `wt` v0.80.0 (source checkout `max-sixty__worktrunk` @ `f98bd11816e040c8782502141bd7b0328083a877`), herdr 0.9.3, git 2.50.1 (Apple), macOS. Scratch repo `/tmp/cpd-trials/t3/repo` (primary checkout on `main`) and a clone `/tmp/cpd-trials/t3/repo2` (used for setup-failure and gate-bypass tests). Every `wt` command ran with `HOME=/tmp/cpd-trials/t3/home XDG_CONFIG_HOME=…/home/.config`. herdr commands ran with the real HOME (see Finding 0).

## Verdict

- **`wt` can do the whole lifecycle non-interactively. All must-items pass.**
  - Creation is `wt --yes -C <lead-wt> switch --create <br> --base <lead> --no-cd --format json </dev/null`. It blocks on `pre-start`, prints one JSON object on stdout, and exits 0.
  - Merge is `wt --yes -C <attempt-wt> merge <lead> --no-squash --no-remove --format json </dev/null`. It rebases, runs the `pre-merge` gate, then does a CAS fast-forward.
  - Commits are kept: original authors and messages, new SHAs, linear history.
- **The Spec's branch naming is impossible in git.** A Lead Branch `cpd/demo` and an Attempt branch `cpd/demo/j1-a1` cannot coexist: `refs/heads/cpd/demo` is a file, so it cannot also be a directory. Use a flat suffix, e.g. `cpd/demo--j1-a1`. Wherever this report says `cpd/demo--j1-a1`, the Spec said `cpd/demo/j1-a1`. The convention must be fixed in the cpd design.
- **`wt merge` exit codes do not distinguish failure kinds.**
  - Conflict, CAS lost and dirty target all exit 1 with empty stdout.
  - A hook failure propagates the hook's own exit code.
  - Fix: wrap the gate as `./verify.sh || exit 64`, so gate-failed is exit 64 (verified). Classify exit 1 by probing state: `rebase-merge` dir exists → conflict; the Lead ref moved since the snapshot → cas-lost; stderr text "conflicting uncommitted changes" → dirty-target. The stderr strings are unverified as stable across versions, so pre-check where possible.
- **Safety properties hold.**
  - Gate failure, CAS loss and dirty-overlap all left the Lead ref and checkout unchanged. Ref and tree move together or not at all (source `push.rs` `advance_target`, rollback by second CAS).
  - A rebase conflict is left *in the Attempt worktree*, never in the Lead checkout. `git rebase --abort` restores it exactly.
- **Put the Lead Branch in its own wt worktree** (`repo.cpd-demo`), not in the User's main checkout. The primary checkout stays on `main`, the User's own work never collides with merges, and `wt merge` syncs the Lead worktree via `read-tree -m -u`. Dirty overlap is refused; non-overlapping dirt is carried through. `cpd` should still pre-check the Lead worktree is clean.
- **Footguns `cpd` must handle:**
  - `wt remove` keeps the branch as "unmerged" (it compares against `main`, not the Lead Branch). Follow with `git branch -d`.
  - `wt remove` does not close the herdr workspace; close it explicitly.
  - A failed `pre-start` exits with the hook's code but leaves the half-created worktree and branch behind.
  - **A Worker can weaken the Gate**: `.config/wt.toml` and `verify.sh` are read from the Attempt worktree (verified by bypass, Finding 11). Reject Attempts whose diff touches them.
  - Never leak the fake/other `HOME` to herdr (Finding 0).

## Acceptance checklist

| # | Item | Result | Evidence |
|---|---|---|---|
| 1 | must: create from Lead HEAD, merge-base proof, blocking setup, non-interactive form, exit codes | pass (name deviates, see Verdict) | F1, F2 |
| 2 | must: herdr registration, `worktree list` and `workspace list` | pass | F3 |
| 3 | must: 2 commits, `j2-a1` merged first, `--no-squash --no-remove`, linear graph, Lead checkout updated and clean | pass | F4 |
| 4 | must: gate failure leaves Lead ref and checkout unchanged | pass | F5 |
| 5 | must: rebase conflict state, detection, abort recipe | pass | F6 |
| 6 | must: concurrent merges, exactly one wins, rerun succeeds | pass (one run; won/lost order is a race) | F7 |
| 7 | should: dirty Lead checkout | pass | F8 |
| 8 | should: cleanup semantics, herdr side | pass | F9 |
| 9 | should: PR rebase onto advanced `main`, conflict surfacing | pass | F10 |
| 10 | should: Gate after merge, recipe and cost | pass (trivial gate, so cost is only the wrapper overhead) | F11 |
| extras | Recipes, minimal `wt.toml` | done | Recipes |

## Findings

### F0. Environment isolation pitfall
herdr locates its session socket from `HOME`/`XDG_CONFIG_HOME`. With the fake HOME exported, herdr says:
```
{"error":{"code":"server_not_running","message":"no herdr server is running at /tmp/cpd-trials/t3/home/.config/herdr/sessions/cpd-trial/herdr.sock; …"}}
```
I ran herdr as `env -u XDG_CONFIG_HOME HOME=/Users/claytonduarte herdr --session cpd-trial …`. For `cpd`: do not scope HOME globally; pass `wt`/`herdr` the environment each needs.

`wt` wrote **nothing** under `$XDG_CONFIG_HOME` (`find home -type f` finds only python's `.pyc` cache from my own `python3` calls). `--yes` does not persist approvals, and the real `~/.config/worktrunk` does not exist. Per-repo state lives in `<repo>/.git/wt/{cache,logs/commands.jsonl,trash}`.

### F1. Create from Lead HEAD, blocking setup
Setup: repo with `.config/wt.toml` (`pre-start` = `sleep 5 && date +%s.%N > .setup-done && cp '{{ primary_worktree_path }}/.env' .env`, `pre-merge` = `./verify.sh`). Lead Branch `cpd/demo` checked out in its own worktree `repo.cpd-demo`, created with `wt --yes switch cpd/demo --no-cd` (5.2 s, because `pre-start` ran there too).

Without `--yes`, no TTY:
```
▲ repo needs approval to execute 1 command: … ✗ Cannot prompt for approval in non-interactive environment
↳ To skip prompts in CI/CD, add --yes; to pre-approve commands, run wt config approvals add --yes      → exit 1
```
So `--yes` is mandatory, and it is global, so it goes before the subcommand.

Creating the Attempt after the Lead gained its own commit (`7896da2`):
```
start=1791844074.370
$ wt --yes switch --create cpd/demo--j1-a1 --base cpd/demo --no-cd --format json </dev/null   → exit 0
end=1791844079.700
stdout: {"action":"created","branch":"cpd/demo--j1-a1","path":"/private/tmp/cpd-trials/t3/repo.cpd-demo--j1-a1","created_branch":true,"base_branch":"cpd/demo"}
.setup-done = 1791844079.615   (before return at 079.700)   .env = SECRET=1 (copied from the primary checkout)
git merge-base cpd/demo cpd/demo--j1-a1 = 7896da2f…  == git rev-parse cpd/demo
```
- `pre-start` blocks. The command returned 5.33 s after start, 85 ms after `.setup-done` was written.
- Progress goes to stderr, JSON to stdout, and there is no `cd` integration (`--no-cd`).
- `--base cpd/demo` is mandatory; the default base is `main`.
- The `path` is the realpath (`/private/tmp/…`); use it verbatim.
- Six parallel creations (`j3..j8`) with hooks all exited 0. Re-running `--create` on an existing branch exits 1 ("Branch … already exists"). `wt switch <existing> --format json` exits 0 with `{"action":"existing",…}`, which is useful for idempotent retries.
- Setup failure (repo2, `pre-start = "echo setting up; exit 3"`): `wt` exits **3** (the hook's code), stdout is empty, and **the worktree and branch remain** (`git worktree list` shows `repo2.cpd-x`). `cpd` must clean up with `wt remove <br> -D -f --foreground`.

### F2. Merge-base proof
Covered in F1. Every later Attempt was created with `--base cpd/demo`, and `git branch -v` showed all at the Lead HEAD (`1e8c71b`) at creation.

### F3. herdr registration
```
$ herdr --session cpd-trial workspace create --cwd /tmp/cpd-trials/t3/repo --label t3 --no-focus   → w2 (primary, label t3)
$ herdr --session cpd-trial worktree open --workspace w2 --path /private/tmp/cpd-trials/t3/repo.cpd-demo--j1-a1 --label j1-a1 --no-focus
  → {"already_open":false,…,"workspace":{"workspace_id":"w3","label":"j1-a1","worktree":{"checkout_path":"…j1-a1","is_linked_worktree":true,"repo_root":"/tmp/cpd-trials/t3/repo",…}}}
$ herdr … worktree list --workspace w2 | jq -c '.result.worktrees[]|{branch,open_workspace_id}'
  {"branch":"main","open_workspace_id":"w2"}
  {"branch":"cpd/demo","open_workspace_id":null}
  {"branch":"cpd/demo--j1-a1","open_workspace_id":"w3"}
$ herdr … workspace list | jq -c '.result.workspaces[]|{workspace_id,label,worktree}'
  … {"workspace_id":"w3","label":"j1-a1","worktree":{"checkout_path":"…j1-a1","is_linked_worktree":true,"repo_key":"/private/tmp/cpd-trials/t3/repo/.git",…}}
```
- `worktree open --cwd <repo-root> --path <wt>` works without knowing the parent workspace id; I verified it for `j2-a1`, which became `w4`.
- Opening is idempotent: a second `open` returns `"already_open":true` and the same id.
- herdr's `worktree create` uses plain `git worktree add`, so it would skip `wt` hooks. Always create with `wt`, then `open`.
- `worktree list` matches paths as realpaths (`/private/tmp/…`).

### F4. Rebase + fast-forward, commits kept
`j2-a1` (1 commit) merged first; then `j1-a1` (2 commits, authored "Worker One") was merged. Its base was stale, so it had to rebase.
```
$ wt --yes -C repo.cpd-demo--j2-a1 merge cpd/demo --no-squash --no-remove --format json </dev/null → exit 0
   {"branch":"cpd/demo--j2-a1","committed":false,"rebased":false,"removed":false,"squashed":false,"target":"cpd/demo"}
before j1: a51fdd5 (first), 71d0494 (second)
$ wt … -C repo.cpd-demo--j1-a1 merge cpd/demo --no-squash --no-remove --format json → exit 0   (… "rebased":true …)
  ◎ Rebasing onto cpd/demo… ✓ Rebased → pre-merge project:verify → "Merging 2 commits to cpd/demo @ 1e8c71b" → ✓ Merged
$ git log --graph --format='%h %an <%ae> %s'   (in the Lead worktree)
* 1e8c71b Worker One <w1@example.com> j1: second commit      (was 71d0494)
* 1f2cd4d Worker One <w1@example.com> j1: first commit       (was a51fdd5)
* f16dc7d Worker Two <w2@example.com> j2: add j2.txt
* 7896da2 CPD Trial … lead: add lead.txt
…
$ git status -sb (lead worktree) → "## cpd/demo" (clean); j2.txt present; ref 1e8c71b
```
Linear, original authors and messages, new SHAs. The Lead checkout was updated by the same operation and stayed clean. Neither merge commits uncommitted work, since the Attempts were committed.
- JSON on stdout has only booleans (`rebased`, `committed`, `squashed`, `removed`) and no SHAs, so `cpd` reads the SHAs with git.
- **Unverified:** with `--no-squash` and a dirty Attempt tree, `wt merge` is documented to *commit* uncommitted changes (`--stage all` by default). I did not test it. `cpd` should require a clean Attempt tree (`git status --porcelain`, ignored files excluded) before merging, or pass `--no-commit` (documented to require a clean tree and fail otherwise).

### F5. Gate failure
`j3-a1` with a committed change plus ignored `FAIL_GATE`:
```
$ wt --yes -C repo.cpd-demo--j3-a1 merge cpd/demo --no-squash --no-remove --format json </dev/null  → exit 1, stdout empty
◎ Running pre-merge project:verify … verify: FAIL_GATE present
✗ pre-merge command failed: verify: exit status: 1
↳ To skip pre-merge hooks, re-run with --no-hooks
Lead ref 1e8c71b… unchanged; lead status: [] (clean); attempt keeps its commit, clean.
```
The hook's exit code propagates: with `verify.sh` changed to `exit 42`, `wt hook pre-merge` and `wt merge` both exited **42** (source: `command_executor.rs`, `HookCommandFailed` → `exit_code()`). With the wrapper `verify = "./verify.sh || exit 64"` (repo2) the gate failure exits **64** (verified). Use that to make gate-failed unambiguous.

### F6. Rebase conflict
`j4` and `j5` both edited line 2 of `app.txt`. `j4` merged. `j5`:
```
$ wt --yes -C repo.cpd-demo--j5-a1 merge cpd/demo --no-squash --no-remove --format json → exit 1, stdout empty
◎ Rebasing onto cpd/demo… ✗ Rebase onto cpd/demo incomplete … CONFLICT (content): Merge conflict in app.txt
```
- State: the Attempt worktree is **mid-rebase** (`git status`: "interactive rebase in progress; onto ea76315", `both modified: app.txt`). The Lead ref `ea76315` and Lead checkout were untouched.
- Detection:
  - `git -C $A diff --name-only --diff-filter=U` lists `app.txt`.
  - `test -d "$(git -C $A rev-parse --git-path rebase-merge)"` is true. Also check `rebase-apply` for completeness (unverified).
- Abort: `git -C $A rebase --abort` → exit 0; HEAD back to the original `221e3a1`, no rebase dir, clean.
- `wt step rebase <lead>` alone behaves the same (exit 1, rebase open).
- The Worker then resolves by rebasing itself and re-requesting merge.

### F7. Concurrent merges (CAS)
`j6` and `j7` (disjoint files) started with `&` at the same moment:
```
j6: exit 0   Merged to cpd/demo (1 commit)  → Lead 842ad30
j7: exit 1   ✗ Can't push to local cpd/demo branch
   Failed to update ref: … cannot lock ref 'refs/heads/cpd/demo': is at 842ad30… but expected ea76315…
Lead: 842ad30 on ea76315; lead worktree clean (j6.txt present). j7 attempt: clean, rebased onto the old tip.
rerun j7: exit 0 → "Rebasing onto cpd/demo… ✓ Merged" → Lead 4c2b0cd (linear)
```
Exactly one wins; the loser is a clean refusal (CAS `update-ref <new> <old>`), no partial state. Rerunning re-rebases and succeeds. This was one race outcome. The source also has a second path ("not fast-forward", `NotFastForward`) when the Lead moves before the target snapshot; I did not trigger it. It is also exit 1.
- Both losing paths exit 1, so `cpd` tells cas-lost apart by "Lead ref ≠ ref snapshotted before the call, no rebase in progress".
- `cpd` should still serialise merges itself with a per-repo lock; CAS is the safety net.
- Note the loser pays the full gate cost before it loses.

### F8. Dirty Lead checkout
`j8` touches `app.txt` and adds `j8.txt`.
- (a) Lead has an uncommitted edit to `app.txt` (overlap): exit 1, after rebase and the gate ran, with
  ```
  ✗ Can't push to local cpd/demo branch: conflicting uncommitted changes
    app.txt
  ↳ Commit or stash these changes in /private/tmp/cpd-trials/t3/repo.cpd-demo first
  ```
  The Lead ref was unchanged (rolled back), and the dirty file was untouched.
- (b) Lead has an uncommitted edit to `README.md` plus an untracked file (no overlap): **merge succeeds, exit 0**, and the dirt is preserved. The Lead checkout is then still dirty, so this is silent acceptance of what the User/Lead should never have done.
- (c) Lead has an *untracked* `j3.txt` that collides with an incoming path: refused like (a), exit 1, ref unchanged, file content kept.
- The gate runs *before* the dirty check, so a dirty target wastes a full gate run. `cpd` should pre-check `git -C $LEAD_WT status --porcelain` is empty (reject if not) rather than rely on `wt`.

### F9. Cleanup semantics
```
$ wt --yes -C repo.cpd-demo remove cpd/demo--j1-a1 --foreground --format json </dev/null → exit 0
✓ Removed … worktree  ↳ Branch unmerged; to delete, run wt remove -D …
[{"branch":"cpd/demo--j1-a1","branch_outcome":"retained_unmerged","kind":"worktree",…}]
```
- The branch was retained because `wt` judges "merged" against the default branch (`main`), not the Lead Branch, even though it is an ancestor of `cpd/demo`. The deletion has to be explicit: `git -C $LEAD_WT branch -d <br>` worked ("Deleted branch … (was 1e8c71b)") and is a native merged-into-Lead-HEAD guard. `wt remove -D` also works but skips the guard.
- herdr side: **`wt remove` did not close the workspace** (`w3` stayed in `workspace list`; its pane cwd followed the removed dir into `.git/wt/trash/repo.cpd-demo--j1-a1-<ts>`). `herdr workspace close w3` → ok. `herdr worktree list` showed the removed worktree gone.
- Without `--foreground`, removal happens in the background (docs, `remove --help`); use `--foreground` for determinism. `.git/wt/trash` was empty afterwards.
- Failed Attempt kept: closing only the herdr workspace (`workspace close`) keeps the checkout; `git worktree list` still shows `j5-a1`, and `worktree list` reports `open_workspace_id: null`. `worktree open` re-opens it (idempotent, F3).
- Closing the primary workspace `w2` while linked workspaces are open fails with `workspace_group_close_required`; close children first (or pass `--group`).
- `herdr worktree remove` is not used: per the docs it only runs `git worktree remove` and never deletes the branch (not tested).

### F10. PR rebase onto `main`
`main` advanced by 2 commits: `d477c19` (add `main1.txt`, clean) and `c8d43ae` (edit line 2 of `app.txt`, conflicts with `j4`'s commit in the Lead Branch).
- Pre-check without touching any worktree: `git merge-tree --write-tree --name-only main cpd/demo` → exit 0 and a tree OID when clean; with both main commits, exit 1 listing `app.txt` plus "CONFLICT (content)". This is a merge-based approximation of what a commit-by-commit rebase will hit.
- Clean case: `wt -C $LEAD_WT step rebase <main-clean>` → "✓ Rebased", exit 0. Linear history, Attempt authors kept, all 8 commits got new SHAs.
- Conflict case: `wt -C $LEAD_WT step rebase main` → exit 1; the **Lead checkout is left mid-rebase** ("Rebasing (5/8)", "interactive rebase in progress"). That violates "never half-merged", so abort immediately (`git -C $LEAD_WT rebase --abort`; verified: HEAD back at the old SHA `59f99f1`, status clean). To resolve, do it outside the Lead checkout; this was tested end to end:
  1. `wt --yes -C $LEAD_WT switch --create <lead>--rebase --base <lead> --no-cd --format json`.
  2. `git -C $R rebase main` → conflict. A Worker (or the User) resolves in `$R`, then `git add` and `GIT_EDITOR=true git rebase --continue`. Here I resolved with a script and the rebase finished ("Successfully rebased", 8 commits).
  3. Run the gate in `$R` (`./verify.sh` → ok).
  4. Move the Lead: `[ "$(git -C $LEAD_WT rev-parse <lead>)" = "$OLD" ] && [ -z "$(git -C $LEAD_WT status --porcelain)" ] && git -C $LEAD_WT reset --keep $NEW`. This rewrote the Lead; `git merge-base --is-ancestor main cpd/demo` → yes; the checkout was clean. The `reset --keep` step is not CAS-atomic against a concurrent merge, so `cpd` holds its merge lock around it.
  5. `wt --yes -C repo remove <lead>--rebase --foreground -D` (first attempt failed with "not a git repository" because I ran from a non-repo cwd; always pass `-C <repo>`).
- Live Attempts based on the old Lead SHA are unaffected; they rebase at their own merge time.

### F11. Gate after merge, and Gate bypass
- Post-merge gate on the Lead worktree: `wt --yes -C $LEAD_WT hook pre-merge </dev/null` re-runs the configured `verify` hook there (exit 0 = ok, exit code of the hook otherwise; 0.27 s here with a trivial script vs 12 ms for `./verify.sh` directly). Cost is the gate's duration in all cases. It is paid once in the Attempt (`pre-merge`, after the rebase) and again on the Lead.
  - *Inference, not demonstrated:* since the FF is CAS-guarded and the Lead tree equals the tree the Attempt gate just tested, the second run adds assurance only against env drift or a weakened gate. `cpd` can decide to run it only after rebased merges or at PR time.
  - `post-merge` hooks are background and cannot fail the merge, so they are unsuitable as a gate.
- Rollback demo: `j9` merged, then `FAIL_GATE` made the Lead gate fail (exit 1); `git -C $LEAD_WT reset --keep $PRE` restored the pre-merge HEAD (`fb77380`), checkout clean. This rewrites an unpublished Lead Branch; acceptable pre-PR.
- **Gate bypass (verified, repo2):** an Attempt that commits `.config/wt.toml` with `verify = "true"` merges to `main` with `FAIL_GATE` present (exit 0; the weakened hook ran). `wt` reads project config from the Attempt worktree being merged. A post-merge Lead gate would also read the merged config. Mitigation: before merging, reject when `git diff --name-only <lead>...<br>` contains `.config/wt.toml` or the gate script (or require the Reviewer to approve explicitly). Run the post-merge gate with the *Lead's pre-merge* config, e.g. `git -C $LEAD_WT show $PRE:.config/wt.toml` (not tested).

## Recipes

Conventions: `REPO` = primary checkout; `LEAD` = Lead Branch (e.g. `cpd/demo`); `LEAD_WT` = Lead worktree path; `BR="$LEAD--j1-a1"` (flat suffix, **not** `$LEAD/j1-a1`, which git rejects). Every call: `</dev/null`, `--yes` (global, before the subcommand), `-C <dir>`. Do not run `wt` and `herdr` under a rewritten HOME unless the server uses it too.

### Minimal per-repo `.config/wt.toml`
```toml
[pre-merge]
verify = "./verify.sh || exit 64"   # Gate. 64 = gate-failed, distinguishable from wt's generic exit 1

# Only if the repo needs per-worktree setup (runs blocking before the Worker starts):
[pre-start]
setup = "cp '{{ primary_worktree_path }}/.env' .env && npm ci"
```
`worktree-path` is deliberately unset: the default `../<repo>.<branch-sanitized>` is fine, and `cpd` must read `.path` from the JSON instead of computing it (a User-level `worktree-path` would change it).

### attempt create (blocks until setup is done, i.e. "attempt ready")
```sh
OUT=$(wt --yes -C "$LEAD_WT" switch --create "$BR" --base "$LEAD" --no-cd --format json </dev/null 2>"$LOG"); rc=$?
# rc 0: setup finished; stdout is {"action":"created","branch":…,"path":…}.  "attempt ready" == rc 0.
# rc != 0 (hook's own exit code, or 1): the half-created worktree/branch may exist ->
#   wt --yes -C "$REPO" remove "$BR" -D -f --foreground </dev/null   # then report setup-failed
A=$(echo "$OUT" | jq -r .path)
[ "$(git -C "$A" merge-base "$LEAD" "$BR")" = "$(git -C "$LEAD_WT" rev-parse "$LEAD")" ]   # base assertion
herdr --session "$S" worktree open --cwd "$REPO" --path "$A" --label "$JOB-$ATT" --no-focus | jq -r .result.workspace.workspace_id   # idempotent
# then launch the Worker (pi) in that workspace's root pane
```

### attempt merge
```sh
# preflight (all read-only), under a per-repo merge lock held by cpd
[ -z "$(git -C "$LEAD_WT" status --porcelain)" ]            || exit dirty-target
[ -z "$(git -C "$A" status --porcelain)" ]                  || exit attempt-dirty
git -C "$A" diff --name-only "$LEAD...$BR" | grep -Eq '^(\.config/wt\.toml|verify\.sh)$' && exit gate-tampered
PRE=$(git -C "$LEAD_WT" rev-parse "$LEAD")

wt --yes -C "$A" merge "$LEAD" --no-squash --no-remove --format json </dev/null >"$OUT" 2>"$ERR"; rc=$?
case $rc in
  0)  : success ;;   # then post-gate below
  64) : gate-failed ;;   # Lead untouched; Attempt keeps its commits (it may be rebased already)
  *)  if [ -d "$(git -C "$A" rev-parse --git-path rebase-merge)" ] || [ -d "$(git -C "$A" rev-parse --git-path rebase-apply)" ]; then
        git -C "$A" rebase --abort                      # conflict: restore the Attempt; Lead untouched
        : conflict
      elif grep -q 'conflicting uncommitted changes' "$ERR"; then : dirty-target      # Lead ref rolled back, unchanged
      elif [ "$(git -C "$LEAD_WT" rev-parse "$LEAD")" != "$PRE" ]; then : cas-lost      # rerun the same command
      else : unknown-failure   # includes failures with hook exit codes other than 64
      fi ;;
esac
# on success: post-gate on the Lead
wt --yes -C "$LEAD_WT" hook pre-merge </dev/null || { git -C "$LEAD_WT" reset --keep "$PRE"; : post-gate-failed; }
```
Exit-code table: `0` success (JSON `rebased` true/false), `64` gate-failed (via wrapper), `1` = conflict / cas-lost / dirty-target (probe as above), or the hook's code for pre-start/pre-merge hook failures, `128+n` signal. The `grep` on stderr is the only text dependency (unverified across versions).

### attempt remove
```sh
# merged (approved) Attempt
WS=$(herdr --session "$S" worktree list --cwd "$REPO" | jq -r --arg p "$A" '.result.worktrees[]|select(.path==$p)|.open_workspace_id // empty')   # $A is wt's realpath
[ -n "$WS" ] && herdr --session "$S" workspace close "$WS"
wt --yes -C "$LEAD_WT" remove "$BR" --foreground --format json </dev/null     # removes the worktree; branch retained ("unmerged" vs main)
git -C "$LEAD_WT" branch -d "$BR"                                              # merged into the Lead's HEAD -> guarded delete
# failed Attempt: only close the herdr workspace; keep the worktree and branch. Re-open with `worktree open`.
```
Close order: I verified each step, but closing the workspace *after* `wt remove` was what I ran; closing first is the order I recommend (unverified).

### pr rebase
```sh
[ -z "$(git -C "$LEAD_WT" status --porcelain)" ] || exit dirty-lead
if git merge-tree --write-tree --name-only main "$LEAD" >/tmp/mt 2>&1; then          # exit 0: clean
  wt --yes -C "$LEAD_WT" step rebase main </dev/null || { git -C "$LEAD_WT" rebase --abort; exit rebase-failed; }
else                                                                                  # exit 1: conflicts, file names in /tmp/mt
  OLD=$(git -C "$LEAD_WT" rev-parse "$LEAD")
  wt --yes -C "$LEAD_WT" switch --create "$LEAD--rebase" --base "$LEAD" --no-cd --format json </dev/null   # R = .path
  git -C "$R" rebase main   # conflict -> hand $R to a Worker to resolve+continue; then gate in $R:
  (cd "$R" && ./verify.sh)
  NEW=$(git -C "$R" rev-parse HEAD)
  [ "$(git -C "$LEAD_WT" rev-parse "$LEAD")" = "$OLD" ] && [ -z "$(git -C "$LEAD_WT" status --porcelain)" ] && git -C "$LEAD_WT" reset --keep "$NEW"
  wt --yes -C "$REPO" remove "$LEAD--rebase" --foreground -D </dev/null
fi
```
The Lead worktree is never left mid-rebase: any `step rebase` failure is aborted immediately.

## Risks and unknowns

- **Gate bypass via `.config/wt.toml` / `verify.sh` edits** (F11). It needs a path guard or Reviewer attention. Also any other file the gate executes (the gate script's dependencies) is Worker-controlled; unverified how far to take this.
- **Exit-code ambiguity.** Only gate-failed can be made distinct (wrapper → 64). Everything else relies on state probes plus one stderr substring. wt-version upgrades could change the text.
- **Uncommitted changes in the Attempt at merge time**: unverified (`--stage all` may auto-commit them with a generated message).
- **`wt remove` for the Attempt while a pi process still runs in its pane**: the pane cwd followed into `.git/wt/trash`; `--reap` (kill processes) exists but was not tested. Close the herdr workspace first.
- **Hook approvals**: `--yes` skips approval on every call, so a changed `pre-start` hook in an Attempt branch executes with no prompt. Combine with the path guard. `wt config approvals add --yes` persists approvals in `~/.config/worktrunk` (not tested; would write to the User's real config).
- **Real user config** may set `worktree-path` or hooks that add steps; the trial used an empty user config. A user-level `pre-start`/`pre-merge` would run first (source `hook.md`: user hooks run before project hooks), so a user hook failing aborts before the project's hook. Not tested.
- The CAS race was observed once (`update-ref` path). The `NotFastForward` path was not triggered.
- `merge-tree` conflict prediction is a merge, not a replay of the 8 commits; a clean merge-tree could still conflict commit by commit (expected rare; `wt step rebase` result is the authority).
- Timing measurements are trivial-gate numbers only; real Gate duration dominates and is paid up to twice per merge, plus once more per CAS retry.

## Escalations

None. Nothing required an install, credentials, or leaving the isolation rules. Design decision needed from the Lead (not a blocker for the trial): the Attempt branch naming scheme, since `<lead>/<job>-<attempt>` is impossible. Recommend `<lead>--<job>-<attempt>`.

Cleanup done: herdr workspaces `w2`, `w4`, `w6` (and the closed `w3`, `w5`) removed; `cpd-trial` server and other trials' workspace `w1` left alone; no background processes. Scratch worktrees remain under `/tmp/cpd-trials/t3/` (not in any real repository). Files `wt` wrote under the fake config dir: none.
