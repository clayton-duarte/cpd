# Merges, Gates, pushes and clean-ups without the per-repo lock

Job `nl-design` of Plan `cross-repo`, for the User's review. The User's decision (hs-d243): no lock between PRs or Plans; work that conflicts across PRs is a planning problem (`needs`), not something cpd serialises; even the exclusion inside one PR is designed away where possible. This file proposes; it changes no code and no contract.

## Answer first

- **Between two PRs the lock guards nothing that git does not already guard**, except two pieces of shared git state that git does *not* retry on: `.git/config` (cpd writes it at a Lead worktree's creation and at `push -u`) and a ref's lock file, which git retries for only 100 ms. Both already race today outside the lock (PR starts are not locked). Fix both and nothing between PRs needs cpd's help: no `.git/config` writes (`--no-track`, no `-u`), and cpd's git calls wait up to 10 s for a ref lock (`core.filesRefLockTimeout`).
- **Inside one PR no lock is needed either.** The state that replaces it already exists or is per invocation: the compare-and-swap (CAS) on the Lead Branch ref (git), the Ledger CAS on `attempts.status = 'merging'`, the per-Attempt driver lock, the PR's status in the database, and a **throwaway gate worktree** per Gate run. The Lead worktree becomes a read-only view that whoever moves the Lead Branch brings up to date, in a way that ends in the same state whatever the order (**rule S**, §2.3).
- `RepoLock`, `locks/repo-<sha1>.lock`, `timeouts.lock_wait_seconds` and the `sha1` crate are deleted; `lock_timeout` stays (the `pr-poll` lock still uses it).
- 7 Jobs (§3.3). The risk that needs a measurement is the Gate's cost in a fresh worktree (§4, risk 1).

## Method and evidence

- Read on `origin/main` = `b0e09f4`. The branch base `6068cd0` differs from it in 16 files, none under `core/src/{merge,pr,git,watch,driver}` (`git diff --stat HEAD origin/main`), so every `file:line` below holds on both.
- git 2.43.0. Docs quoted from the installed man pages: git-update-ref(1), git-config(1), git-gc(1), git-worktree(1), git-branch(1), git(1).
- **Experiments I ran** (in `/tmp`, a bare `origin` and a repo with linked worktrees; deleted afterwards). Each claim below marked "(run)" comes from these:
  1. `update-ref <ref> <new> <stale-old>`: exit 128, `cannot lock ref … is at X but expected Y`.
  2. `refs/heads/x.lock` present: `update-ref` fails after ≈ 0.15 s (`real 0m0.147s`) with `Unable to create … x.lock: File exists`, which matches `core.filesRefLockTimeout`'s 100 ms.
  3. `refs/remotes/origin/main.lock` present and the remote moved: `git fetch origin main` exits 1, `cannot lock ref 'refs/remotes/origin/main'`, `(unable to update local ref)`; the tracking ref stays old.
  4. `.git/config.lock` present: `git branch --set-upstream-to` exits 1, `git config` exits 255, `could not lock config file`; no wait.
  5. **12 parallel `git worktree add -b t<i> <path> origin/main`**: 10 exited 255 (`could not lock config file … unable to write upstream branch configuration`). The 10 failures left **their branches behind with no worktree**, and some left a half-written `branch.<name>` section (`branch.t8.remote` without `branch.t8.merge`).
  6. 16 parallel `git worktree add --detach <path> <sha>` and 16 parallel `git worktree add -b n<i> <path> <sha>` (no tracking): all exited 0.
  7. `index.lock` present in a linked worktree: `read-tree -m -u` fails at once (exit 128); `git status --porcelain` still exits 0.
  8. `git rev-parse --git-path` in a linked worktree: `FETCH_HEAD`, `index`, `ORIG_HEAD` are under `.git/worktrees/<id>/`; `config`, `packed-refs`, `logs/refs/heads/<b>`, `gc.pid` are in the common dir.
  9. A push of an ancestor after its descendant was pushed: exit 1, porcelain `!	<sha>:refs/heads/lead	[rejected] (non-fast-forward)`.
  10. `gc.pid` naming a live pid on this host: `git gc` exits 128 `gc is already running`; `git gc --auto` exits 0 and does nothing.
- Anything not run or read is marked "unverified" or "I infer".

## 1. Inventory

### 1.1 Every holder of the lock today

| # | Operation | Takes it | Holds it over | The race it guards | Between two PRs? |
|---|---|---|---|---|---|
| L1 | Driver A8, the merge | `core/src/merge/mod.rs:71` `RepoLock::wait`, no limit; released at `:88` after the step-10 transaction `:78-87` | R for the repo's orphans `:72`; step 3 Lead worktree `status` `:125`; tamper check `:143-148`; rebase in the Attempt worktree `:151-156`; CAS `update-ref` `:166`; push `:169`; `read-tree -m -u old new` in the Lead worktree `:171` | (a) two merges of one PR interleaving between snapshot `:123` and CAS; (b) their step-9 syncs of the one Lead worktree (index lock, `read-tree old new` in the wrong order); (c) a merge moving the Lead Branch and the Lead worktree under that PR's Gate (L3–L5); (d) `pr rebase` rewriting the Lead Branch after this merge's CAS and before its step 10, which breaks R's ancestry test; (e) a dead driver's stale Lead worktree failing step 3 as `dirty_target` (hence "R first", contract §7.4 "Who runs R") | **No.** Each PR has its own Lead Branch ref and Lead worktree; the Attempt worktree is the Attempt's own. Shared state: §1.2 |
| L2 | R from engine E2 | `core/src/watch/liveness.rs:160` `try_take`, skipped when held | `merge::recover` `:164` → `recover.rs:31-60`, per orphan `recover_one` `:64-133`: R1 ancestry `:67-76`, R2 `finish_sync` `:80` (`:139-177`), the step-10 transaction `:83-89`, push `:92`, `rest_of_a9` `:95` (`:216-229`), R3 `:99-112` | R running twice is **already** excluded without it: R takes the dead driver's lock (`recover.rs:50`, `take_free_driver_lock`, `lock.rs:111-114`) and `end_merging` is a CAS on `status = 'merging'` (`ledger.rs:206-214`, `cas_lost` on 0 rows). The repo lock adds only: R's `finish_sync` against a live merge's step 9 on the same Lead worktree, and R's verdict against a `pr rebase` rewrite (L1 d) | **No**: R for PR A touches PR A's ref and worktree |
| L3 | `pr open` | `core/src/pr/mod.rs:259-285` `lock()`: `wait_for` `lock_wait_seconds` `:260-261`, then R when an orphan exists `:263-283`; taken at `open.rs:34`, released `:45` | re-read and re-check `:35-39`; `require_clean` `:42`; the Gate in the Lead worktree `:43` (`mod.rs:307-338`, `run_hook(… lead …)` `:311`); `push -u origin <lead>` `:44` | a Job merging (appended Jobs are queued at once) during the Gate: files change under the Gate and the push publishes the ref as it is then, not the gated commit; two `pr open` of one PR (`gh pr create` twice); an unrecorded merge at Gate time | **No**, except `push -u` writes `.git/config` (§1.2) |
| L4 | `pr rebase` | `rebase.rs:26`, released `:62` | `require_clean` `:30`; `fetch origin <base>` `:31`; `merge-tree` `:38`; `git rebase` **in the Lead worktree** `:54`; the Gate `:59`; `push --force-with-lease` `:60-61` | as L3, plus: the rebase rewrites the checked-out branch while merges CAS and `read-tree` in the same worktree; a rewrite over a CAS'd but unrecorded merge (L1 d) | **No**, except `fetch origin <base>` updates `refs/remotes/origin/<base>`, shared by every PR with that base (§1.2) |
| L5 | `pr push` | `push.rs:23`, released `:49` | `require_clean` `:27`; the Gate `:28`; `push --porcelain origin <lead>` `:31` | as L3 | **No** |
| L6 | Clean-up after a PR merged (#48, hs-d138, hs-d142) | `core/src/pr/poll.rs:353-359` `clean_locked`, `try_take`, the PR skipped when held; called at `:289-290` (earlier PRs) and `:309` (just merged) | `clean` `:364-382`: `discard_worktree` per Attempt `:369`, `remove_merged_lead` `:375` | hs-n124: a Lead worktree removed mid-Gate; also an Attempt worktree removed while that Attempt's merge uses it | **Needlessly yes**: a Gate of PR B holds the lock and so delays PR A's clean-up, because the lock is per repo |

The rest of the lock: `core/src/merge/lock.rs:17-100` (`RepoLock`), `core/src/home.rs:114-117` (`repo_lock` path), `core/src/config/file.rs:120-121, 246` (`lock_wait_seconds`). Contract: §1.4 line 90, §7 intro line 1720, §7.2 line 1747, §7.3 line 1759, §7.4 lines 1801, 1812, 1816, 1826-1827, 1830, §7.6 lines 1849, 1865, 1873, §8.2 line 1925. commands.md: lines 1055, 1135, 1141, 1182, 1215, 1221, 1398, 1404. implementing.md line 115 (`sha1`).

**Not under the lock, on the same shared git state, today.** PR start `fetch` and `worktree add` (`core/src/git/worktree.rs:223, 237, 370`, from `plan lock`, E7 and `pr start`); Attempt worktree creation A2 (`worktree.rs:426`); A9's removal of a merged Attempt's worktree, which runs after the lock is released (`merge/mod.rs:88`, then `driver/run.rs:517-519` → `cleanup` `:530-534`); the User's and the Lead's own git commands; `git gc --auto`. cpd already relies on git's own locking for these.

### 1.2 Shared git state between two PRs of one repo

| State | Written by cpd | What git does | Needs cpd? |
|---|---|---|---|
| Object store | every commit, rebase, fetch, `write-tree` | Content-addressed; `gc` prunes only unreachable objects older than `gc.pruneExpire` (2 weeks), and git-gc(1) NOTES says: "users who run commands concurrently have to live with some risk of corruption (which seems to be low in practice)". `gc --auto` runs after "common porcelain operations that create objects" (git-gc(1)), so from any git command, never covered by the lock | No. Unchanged by removing the lock |
| `refs/heads/<lead branch>` | merge CAS, `pr rebase` | One lock file per ref; `update-ref <ref> <new> <old>` stores new "after verifying that the current value of the <ref> matches <oldvalue>" (git-update-ref(1)); stale old → exit 128 (run 1) | No: one ref per PR |
| `refs/remotes/origin/<base>` | `fetch origin <base>`: PR start (`worktree.rs:223`), `pr rebase` (`rebase.rs:31`) | The ref's lock, retried for `core.filesRefLockTimeout`, "Default is 100" ms (git-config(1)); a second fetch fails exit 1 (run 3) | **Yes, a real race today** (PR start is unlocked): two PRs with the same base fetching at once. Fix G2 |
| `refs/remotes/origin/<lead branch>` | each push of that branch | the same ref lock | Per PR only |
| `packed-refs` | ref deletes (`branch -D` in the clean-up, `branch -d` in A9) and `gc`'s `pack-refs` | `packed-refs.lock`, retried for `core.packedRefsTimeout`, default 1000 ms (git-config(1)) | No; G2 raises it too |
| `.git/worktrees/<id>/` | `worktree add`, `worktree remove`, `worktree prune` (`worktree.rs:489`, gc) | one directory per worktree; parallel adds all succeed (run 6). `worktree prune` removes only entries whose directory is gone; I recall git marks an entry `locked` ("initializing") while `worktree add` builds it (unverified) | No |
| `FETCH_HEAD`, `ORIG_HEAD`, `index`, `HEAD` | fetch, rebase, read-tree | per worktree (run 8; git-worktree(1): "all pseudo refs are per-worktree"). The main checkout's `FETCH_HEAD` is shared by every `git -C <repo> fetch`; cpd never reads `FETCH_HEAD` (grep of `core/src`) | No |
| Reflogs `logs/refs/heads/*` | `update-ref -m` | in the common dir, one file per ref (run 8); appended under the ref's lock (I infer) | No |
| **`.git/config`** | `worktree add -b <lead> <path> origin/<x>` sets the upstream (`branch.autoSetupMerge`, git-branch(1)) at `worktree.rs:370`; `push -u` at start (`worktree.rs:181-182` via `:313`) and at `pr open` (`open.rs:44`) | shared by all worktrees (git-worktree(1)); `config.lock` with **no retry** (run 4); 10 of 12 parallel tracking adds fail and leave a branch with no worktree (run 5) | **Yes, a real race today**: two PR starts at once (two Plans locking, E7 and `pr start`); removing the lock adds `pr open`'s `-u`. Fix G1 |
| `gc.pid` | `gc --auto` from any git command | one gc at a time (run 10) | No |
| The remote branch | pushes | a non-fast-forward is rejected (run 9) | Per PR only |

**G1. No `.git/config` writes.** Lead worktrees are created with `worktree add --no-track -b …`, and no push uses `-u`. Nothing in cpd reads an upstream: grep of `core/src` finds no `@{u}`, `upstream` or `--track`. `branch -d` (A9) checks "merged … in HEAD if no upstream was set" (git-branch(1)), and Attempt branches have none. `remove_merged_lead` checks `--remotes=origin`, not the upstream (`worktree.rs:479-494`).

**G2. Ref locks wait instead of failing.** Every git call cpd makes goes through `core/src/git/run.rs:35-40`; add `GIT_CONFIG_COUNT=2`, `core.filesRefLockTimeout=10000`, `core.packedRefsTimeout=10000` to that `Command`'s environment (git-config(1) `GIT_CONFIG_COUNT`). Reason: git's 100 ms default is for interactive use, and cpd now has concurrent writers of `refs/remotes/origin/<base>`. Hooks are spawned by cpd itself (`git/hooks/exec.rs`), not as git's children, so they do not inherit it. Whether `fetch` honours `filesRefLockTimeout` for remote-tracking refs is unverified; the G2 Job tests it with a held `.lock` released after 1 s.

With G1 and G2, nothing between two PRs needs the lock. Run 5 shows G1 is needed today whatever happens to the lock.

### 1.3 The races inside one PR

| Race | Today | Without the lock, unless designed |
|---|---|---|
| R-A Two merges of one PR | serialised | the CAS already rejects the loser (`mod.rs:166`, back to step 2 `:181-195`); **the Lead worktree syncs collide**: `index.lock` (run 7) or `read-tree old new` from a worktree no longer at `old` |
| R-B A merge during that PR's Gate | serialised | files change under the Gate; the pushed ref is not the gated commit; the Gate's untracked artefacts make step 3 fail `dirty_target`, which fails the Job (not a ladder cause) |
| R-C A push of a branch that moved | serialised | `push origin <branch>` publishes whatever the ref is at that moment; two merges' pushes reject each other non-fast-forward (run 9), a spurious `push_failed` |
| R-D `pr rebase` during merges | serialised | `git rebase` rewrites the checked-out branch in the Lead worktree while merges CAS and `read-tree` there; a rewrite over a CAS'd but unrecorded merge makes R1 answer "not an ancestor", so R3 ends a Merged Job `crashed` and the ladder re-runs it |
| R-E The clean-up during a Gate, merge or A9 (hs-n124) | `try_take`, skip | the Lead worktree removed mid-Gate; an Attempt worktree removed while its A9 runs |
| R-F R twice, R against a live driver | driver lock + status CAS (L2) | the same: already safe |
| R-G Two runs of `pr open`, `pr rebase` or `pr push` for one PR | serialised, re-checked after the lock (`open.rs:35-39`) | two Gates, two `gh pr create`, two rebases |

## 2. Design

The rule: **every mutable thing has one owner, or is changed by a compare-and-swap; nothing waits on another PR.**

### 2.1 What replaces the lock

| Race | Replaced by | Not a lock between PRs because |
|---|---|---|
| R-A CAS | the Lead Branch ref CAS (exists); `CAS_RETRIES` stays 3 | per ref = per PR. The default `concurrency = 3` (contract §8.2) bounds how many other merges can move the ref in one window to 2, plus one `pr rebase` |
| R-A sync, R-D sync | **rule S** (§2.3): one sync that ends in the same state whatever the order, run by every mover of the Lead Branch | git's per-worktree `index.lock` only; no cpd lock |
| R-B, R-G Gate | **a throwaway gate worktree per Gate run** (§2.5), detached at the gated commit G | per invocation |
| R-C | **rule P** (§2.4): every push names its commit; "the remote already contains it" is success | per branch |
| R-D unrecorded merge | **guard U** in `pr rebase` (§2.6): before its CAS, no Attempt of this PR may be `merging` with its tip already in the snapshot; dead driver → R, live → wait ≤ 60 s for its step 10 | reads one PR's Attempts in the Ledger |
| R-E | the clean-up checks the database: the PR `merged`, every Attempt `ended` with its driver lock free; it never removes a gate worktree whose pid lives (§2.7) | per PR rows and per Attempt locks |
| R-F | unchanged: driver lock + `end_merging` CAS | per Attempt |

### 2.2 The merge (A8)

Step numbers are kept, as R19 kept step 6.

1. Cancel check. 2. `old = rev-parse refs/heads/<lead>`.
3. **Removed.** The merge never works in the Lead worktree (the rebase is in the Attempt worktree, t3 F6; the fast-forward is a ref CAS). Step 3 existed so that step 9's `read-tree` would not hit local edits (t3 F8). `read-tree -m -u` refuses to overwrite them by itself, and rule S reports a Lead worktree with local changes as `lead_worktree_stale`. Without the lock, step 3 would also misread a concurrent sync's half-written tree as dirty and fail a Job for it. The merge stops producing cause `dirty_target`; the cause stays in the schema and in `spec/mod.rs:41` for old Attempts.
4. Tamper check, 5. rebase, 7. cancel check: unchanged.
8. CAS unchanged; its failure text drops "outside cpd" (other merges and `pr rebase` move the ref now). Then the push, by rule P with `new`.
9. Rule S on the Lead worktree.
10. The transaction, unchanged.

**No R first.** It existed for L1 (e): step 3 is gone and rule S finishes any stale tree. E2 runs R every tick (§2.8).

A review Job's merge (B2) reads `old` only: nothing changes but dropping the lock.

### 2.3 Rule S: the Lead worktree follows its branch, in any order

One function, `sync_lead(lead_worktree, lead_branch)`, called after every move of the Lead Branch by whoever moved it (merge step 9, R2, `pr rebase`) and by the clean-up before its dirty check:

```
up to 20 rounds:
  H = rev-parse HEAD                       # the Lead Branch: the worktree has it checked out
  git diff --quiet  fails                  → local changes: lead_worktree_stale, stop
  T = git write-tree                       # the tree index and worktree agree on
  T == H^{tree}                            → done
  T not the tree of an entry of the Lead Branch's reflog
     (git log -g --format=%T refs/heads/<lead>)  → staged local changes: lead_worktree_stale, stop
  git read-tree -m -u T H
     index.lock held                       → sleep 100–500 ms, next round
     other failure                         → lead_worktree_stale with git's stderr, stop
```

- **Same end state in any order:** every mover runs S after its move and loops until the tree equals the then-current `HEAD`; whoever moves last finishes the job. Two concurrent S runs take turns through git's `index.lock`.
- **Why the reflog check:** today R2 treats "worktree = index ≠ HEAD" as stale (`recover.rs:152-161`) and rewrites it. A Lead's staged edit looks the same and would be lost. S runs at every merge, so it must tell them apart: a stale tree is a previous tip of the branch. `core.logAllRefUpdates` is "true by default in a repository that has a working directory" (git-config(1)); a missing reflog counts as local changes (reported, nothing touched).
- cpd's own `git status` on a Lead worktree (`worktree.rs:481`) gets `--no-optional-locks` (git(1): it stops `status` refreshing the index, so it takes no `index.lock`).
- It replaces step 9's `read-tree -m -u old new` (`mod.rs:171`) and R2's `finish_sync`/`try_finish_sync` (`recover.rs:139-177`).
- Who reports: the mover whose S stopped, one `lead_worktree_stale` Event, as step 9 does today. A sync given up after 20 busy rounds appends nothing: the next mover finishes it. (I infer the Lead's own git commands are the only other users of `index.lock` there.)

### 2.4 Rule P: pushes name their commit

`git push --porcelain origin <sha>:refs/heads/<lead>`, never `-u` (G1):

- **Merge step 8 and R2:** `sha = new`, a plain push. When it is rejected non-fast-forward, read the remote tip `X` (`git ls-remote origin refs/heads/<lead>`). If `merge-base --is-ancestor new X` succeeds, it is success (a later merge already pushed a descendant). Otherwise, or when `X` is not in the local object store, it is `push_failed` as today.
- **`pr open` and `pr push`:** `sha = G`, the gated commit, plain. "Already contained" is success with `pushed: false` and `remote: X` in the output, so the Lead sees when the PR head is past G. Otherwise `pr push` keeps `push_rejected`; `pr open` keeps `git_failed`.
- **`pr rebase`:** `sha = R'` with `--force-with-lease=refs/heads/<lead>:<X0>`, where `X0` is `ls-remote`'s tip read with the snapshot (today the lease reads the tracking ref, `rebase.rs:36`, which other pushes update). Then rule P for a merge with the local tip, when it differs from R' and descends from it, so merges that landed during the Gate reach origin.

### 2.5 The Gate on a snapshot

For `pr open`, `pr push` and `pr rebase`, replacing `gate()`'s run in the Lead worktree (`pr/mod.rs:307-311`):

1. `G` = the commit to gate (the Lead tip read once; for `pr rebase`, R').
2. `W = <repo-parent>/<repo-name>.<plan>.<key>.gate-<pid>`; `git -C <repo> worktree add --detach --lock --reason "cpd gate <pid>" W G`. Detached: no branch and no config write. `--lock` is "the equivalent of git worktree lock after git worktree add, but without a race condition" (git-worktree(1)), so `worktree prune` and a plain `worktree remove` leave it alone.
3. `pre-start` in W (`setup_minutes`, log `run/<plan>/logs/pr-<key>.pre-start.log`), then `pre-merge` (`gate_minutes`, the same log as today). Both are read from W's `.config/wt.toml`, i.e. G's tree: the same "hooks from the tree under test" as today (§7.6). In a detached W the template variable `branch` is absent (`git/hooks/mod.rs:164` reads it from `HEAD`); the Gate Job passes the Lead Branch name as `branch` explicitly, so that `{{ branch }}` keeps its meaning.
4. `git worktree remove --force --force W` ("To remove a locked worktree, specify --force twice", git-worktree(1)), whatever the result.
5. Re-read the PR. If it is no longer in the status the command needs (`dispatch` for `pr open`, `open` for the others), stop with that command's existing refusal (`already` for `pr open`; `illegal_transition` for the others) and push nothing.
6. Push by rule P.

Consequences:
- The Lead worktree's state no longer affects the Gate: `require_clean` (`pr/mod.rs:287-301`) and `dirty_target` leave `pr open`, `pr rebase` and `pr push`. Its reason was "uncommitted edits would be gated but not pushed", and that is gone.
- **Two Gates of one PR at once** each have their own W: no exclusion.
- **Leftovers:** a caller killed mid-Gate leaves a locked W. The next Gate of the same PR and the PR's clean-up remove every `<…>.<plan>.<key>.gate-<pid>` whose pid is not alive (`kill(pid, 0)` → `ESRCH`). pid reuse can keep one alive by mistake until a later sweep (accepted).
- **Cost:** one `pre-start` per Gate (`npm ci` for this repo) and a cold `target/`, with `sccache` when installed (`scripts/cargo.sh`). Unmeasured; see risk 1.
- **Rejected alternative:** one reusable gate worktree per PR (warm caches). It needs an exclusion between two Gates of one PR (a per-PR flock), which is the exclusion hs-d243 asks to design away. It is the fallback if risk 1 materialises.

### 2.6 `pr rebase`

1. The PR is `open`; `fetch origin <base>` (G2 makes it wait for the ref lock).
2. Snapshot: `S = rev-parse refs/heads/<lead>`, `X0 = ls-remote` tip.
3. `merge-tree --write-tree --name-only origin/<base> S`: a conflict is `rebase_conflict`, nothing changed (as today, `rebase.rs:38-51`).
4. `W` as in §2.5, detached at `S`; `git -C W rebase origin/<base>` → `R'` (a detached `HEAD` rebases like a branch, I infer; tested by the Job). An unexpected conflict: `rebase --abort`, remove W, `rebase_conflict`.
5. **Guard U.** For each Attempt of this PR in `merging` whose branch tip is an ancestor of `S` (its CAS happened, its step 10 has not): driver lock free → run R for it (it takes that lock, R-F); held → poll the store every 200 ms until it is `ended`, at most 60 s, else `cas_lost` (exit 3, `details.attempt`; "rerun"). An Attempt whose CAS comes after `S` cannot be lost: its CAS fails against `R'` or ours fails against it.
6. `update-ref -m "cpd: pr rebase <plan>/<key>" refs/heads/<lead> R' S`. If it fails because the ref moved: remove W and go back to 2, at most `CAS_RETRIES` times, then `cas_lost`.
7. Rule S on the Lead worktree.
8. The Gate in W, then the steps of §2.5 from step 4; the push by rule P with the lease `X0`.

What stays the same: on `gate_failed` the rebased branch stays local and is not pushed, and fix Jobs build on it (contract §7.6 step 4), because the CAS of step 6 comes before the Gate. Merges during the Gate CAS onto `R'` (their snapshot is `R'`, or their CAS fails and they retry onto it).

**Rejected:** R decides by the reflog entry `cpd: merge <plan>/<attempt>` instead of ancestry, which would make guard U unnecessary. It ties R's verdict to the reflog's existence and expiry and changes R1, which the contract and `cli/tests/merge.rs:771-863` pin; guard U is a seconds-long wait inside one PR.

### 2.7 The clean-up after a PR's merge

`clean_locked` (`poll.rs:353-359`) becomes `clean_when_idle`, for a PR whose status is `merged` (as today, `poll.rs:328-347`):

- skip the PR, with no Event (a later poll retries, as today), while any of its Attempts is not `ended` or its driver lock is held (probe with no `O_CREAT`, `driver/lock.rs:104-116`): A9 of a driver that just ended may still be removing its worktree;
- remove its gate worktrees whose pid is dead (§2.5);
- run rule S on the Lead worktree first, so a stale tree is not kept as `dirty`; then `remove_merged_lead` as today.

hs-n124 cannot recur, because no Gate runs in the Lead worktree, and a running Gate's W is never removed while its pid lives. Removing the Lead worktree under a `gh` call of a late `pr push` (cwd = Lead worktree, contract §7.6) cannot happen: that Gate's step 5 sees `merged` and stops before any `gh` or push.

### 2.8 R

- **E2** runs R per orphaned Attempt with no repo lock (`liveness.rs:157-171`): `merge::recover` takes `(plan, attempt)` instead of a `RepoLock`. Exclusion is the dead driver's lock (`recover.rs:50`) and the status CAS (`ledger.rs:206-214`), both already there.
- R2 uses rule S (instead of `finish_sync`) and rule P.
- **Nobody else runs R** except guard U. `pr open` (an unrecorded merge leaves its Job `running`, so `jobs_not_done` refuses anyway, `open.rs:155-177`), `pr push` (pushing a recorded or unrecorded commit changes nothing for R) and the merge (§2.2) drop it: `pr/mod.rs:262-283`, `merge/mod.rs:72`.
- Pre-existing, not changed: a driver killed while its `git update-ref` child runs frees its driver lock before git finishes. R can then decide R3 a few milliseconds before the CAS lands (I infer; the driver's fds are close-on-exec, contract §3.4). This was true with the lock too.

### 2.9 Two runs for one PR (R-G)

- `pr open` twice: two Gates (two W), both push G. The `dispatch → open` transaction is a CAS (`open.rs:74-82`): one wins, the other gets `cas_lost`. A second `gh pr create` for the same head should be refused by GitHub (unverified); on any create failure, look the pull request up again (`gh::open_pr`, `gh.rs:196`) and adopt it before failing.
- `pr rebase` twice: the CAS of step 6 picks one; the other goes back to 2 and finds the rebase already done (`R'` contains `origin/<base>`). Its lease `X0` is re-read on each round.
- `pr push` during `pr rebase`: `pr push`'s G is the pre-rebase tip. Its push is rejected and the remote (`R'`) does not contain G, so it is `push_rejected` ("run `pr rebase`"), which is correct.

### 2.10 Deleted

`RepoLock` and its helpers (`merge/lock.rs:17-100`; `DriverLock` and `take_free_driver_lock` stay), `CpdHome::repo_lock` (`home.rs:114-117`), its test `home.rs:365-371` and its line in `no_name_collides_with_cpd_1` (`:380-381`), `timeouts.lock_wait_seconds` (`config/file.rs:120-121, 246`; the User's `~/.cpd/config.toml` does not set it (grep), so its `deny_unknown_fields` does not trip), the `sha1` dependency (used only by `home.rs`, implementing.md:115), and the lock-specific tests (`cli/tests/merge.rs:728-768` and `:1030-1073`, `pr.rs:516`, `pr.rs:1616`, `pr_cleanup.rs:313`; `merge.rs:897` "the merge runs R first" is rewritten for rule S). The two existing `~/.cpd/locks/repo-*.lock` files become inert; nobody needs to delete them.

## 3. Changes and Jobs

### 3.1 Contract (amended first, hs-d4)

- **contract.md:** §1.2 (exit-3 list: `dirty_target` stays, for `attempt`-level history only; `lock_timeout` stays for `pr-poll.lock`); §1.4 (drop the `repo-<sha1>.lock` line); §3.4 (A8 table row and cause `dirty_target`: no longer produced); §3.7 E2 (R without the repo lock); §7 intro (two PRs in one repo share nothing cpd locks); §7.1 (the gate worktree path, and the Lead worktree as a view kept in step by rule S); §7.2 (`--no-track`, no `-u`; the clean-up rules of §2.7); §7.3 (the `pre-merge` row: in a gate worktree; `branch` given explicitly); §7.4 (the lock paragraph, step 3, step 8's push, step 9 = rule S, R2, "Who runs R", the review Job's "under the same per-repo lock"); §7.6 (`pr open`, `pr rebase`, `pr push` steps; guard U; rule P); §8.2 (drop `lock_wait_seconds`); §10 (a traceability row for hs-d243).
- **commands.md:** §6.4 (line 1055), §9.1 (lines 1135, 1141), §9.2 (line 1182), §9.3 (lines 1215, 1221), §9.9 (lines 1398, 1404); §19 if the amendment log needs a line.
- **implementing.md:** line 115 (`sha1`).
- CONTEXT.md: no change (grep: no lock term).

### 3.2 Code

| File | Change |
|---|---|
| `core/src/git/run.rs` | G2: the ref-lock timeouts in the `Command`'s env |
| `core/src/git/worktree.rs` | G1: `--no-track` at `:370`, no `-u` (`push_lead_branch` loses `set_upstream`, `:175-182`, `:313`); rule P (`push_commit`); `--no-optional-locks` at `:481` |
| `core/src/git/lead.rs` (new) | rule S, `sync_lead` |
| `core/src/git/snapshot.rs` (new) | the gate worktree: add, remove, sweep of dead pids |
| `core/src/git/mod.rs` | exports |
| `core/src/merge/mod.rs`, `recover.rs` | §2.2, §2.8; R per Attempt |
| `core/src/merge/lock.rs`, `core/src/home.rs`, `core/Cargo.toml` | delete `RepoLock`, `repo_lock`, `sha1` |
| `core/src/watch/liveness.rs` | E2's R without the lock |
| `core/src/pr/mod.rs`, `open.rs`, `push.rs`, `rebase.rs` | §2.5, §2.6, §2.9; drop `lock()` and `require_clean` |
| `core/src/pr/poll.rs` | §2.7 |
| `core/src/config/file.rs` | drop `lock_wait_seconds` |
| tests | `cli/tests/git.rs`, `merge.rs`, `pr.rs`, `pr_push.rs`, `pr_cleanup.rs`, `config.rs`, `pr_start.rs`, new `cli/tests/no_repo_lock.rs` |

### 3.3 Jobs

One PR, in the cpd repo. Each Job ≤ 90 minutes, one owner per shared file at a time (a later Job edits a file only after the Job that owns it before it is Merged, through `needs`).

| Key | Title | Tier | Files | Needs |
|---|---|---|---|---|
| `nl-contract` | Contract: no per-repo lock (hs-d243) | O3 | `docs/contract.md`, `docs/commands.md`, `docs/implementing.md` | – |
| `nl-git` | git: no config writes, ref locks wait, pushes name their commit | O3 | `core/src/git/run.rs`, `core/src/git/worktree.rs`, `core/src/git/mod.rs`, `cli/tests/git.rs`, `cli/tests/pr_start.rs` | `nl-contract` |
| `nl-sync` | Rule S and the gate worktree | O3 | `core/src/git/lead.rs` (new), `core/src/git/snapshot.rs` (new), `core/src/git/mod.rs`, `cli/tests/git.rs` | `nl-git` |
| `nl-merge` | The merge and R without the lock | O3 | `core/src/merge/mod.rs`, `core/src/merge/recover.rs`, `core/src/watch/liveness.rs`, `cli/tests/merge.rs` | `nl-sync` |
| `nl-pr` | `pr open`, `pr push`, `pr rebase` on a snapshot, with guard U | O4 | `core/src/pr/mod.rs`, `core/src/pr/open.rs`, `core/src/pr/push.rs`, `core/src/pr/rebase.rs`, `cli/tests/pr.rs`, `cli/tests/pr_push.rs` | `nl-merge` |
| `nl-clean` | The clean-up checks the database; delete `RepoLock` and `lock_wait_seconds` | O3 | `core/src/pr/poll.rs`, `core/src/merge/lock.rs`, `core/src/home.rs`, `core/src/config/file.rs`, `core/Cargo.toml`, `Cargo.lock`, `cli/tests/pr_cleanup.rs`, `cli/tests/config.rs`, `cli/tests/merge.rs` (the lock tests only) | `nl-pr` |
| `nl-race` | Tests: two Plans in one repo, a Gate during merges | O3 | `cli/tests/no_repo_lock.rs` (new) | `nl-clean` |

`nl-pr` is the largest (four files and the guard); if its Worker reports it cannot fit, split `pr rebase` (`rebase.rs`, guard U) into its own Job after `pr open`/`pr push`. The chain is sequential because `git/mod.rs`, `cli/tests/git.rs` and `cli/tests/merge.rs` each pass between owners; `nl-race` only reads.

## 4. Risks and what to test

**Risks**

1. **Gate time in a fresh worktree.** `pre-start` plus a cold `target/` on every Gate; for this repo, `npm ci` and a workspace build with `sccache` (when installed). Unmeasured. `nl-pr`'s Worker records the wall time of one real `pre-start` + `pre-merge` in a fresh worktree of this repo against one in a warm Lead worktree and reports it, as information, not as a benchmark. If it is much slower, the fallback in §2.5 needs a User decision.
2. **Ungated commits on the PR.** Unchanged in kind: merges already push after every fast-forward (R35), so an open PR's head is often past its last Gate. What changes: a merge during `pr open`'s Gate no longer waits, so the PR can open with a head past G. The output's `remote` (rule P) makes that visible.
3. **Spurious `push_failed` while a `pr rebase` is between its CAS and its push:** a merge's plain push of `R'+M` against the pre-rebase remote is rejected (run 9). The rebase's own push then pushes the local tip by rule P, so M reaches origin; the Event is noise. Accepted; tested.
4. **More `cas_lost`.** Merges of one PR now race on the CAS; each loss means another merge or a rebase landed. With the default `concurrency = 3`, ≤ 3 losses in a row needs a rebase plus two merges in one rebase window (seconds). `cas_lost` is not a ladder cause, so a Job fails. If it shows up, raise `CAS_RETRIES` rather than add a lock.
5. **Rule S guessing wrong.** A Lead's staged edit whose tree equals an earlier tip's (practically impossible) would be overwritten. A repo with reflogs off reports every stale tree as `lead_worktree_stale` (loud, nothing lost).
6. **`index.lock` held by the Lead's own git commands.** S gives up after 20 busy rounds with no Event; the next mover finishes it. A Lead worktree can then be a merge behind for a while.
7. **Leftover gate worktrees** after a kill: swept by the next Gate of the PR and by the clean-up; one leftover per crash at most.
8. **`gh pr create` twice** (§2.9, unverified that GitHub refuses the second): covered by adopting before failing.
9. **G2 on an older git:** `GIT_CONFIG_COUNT` needs git ≥ 2.31 (I recall; this machine has 2.43). A Mac with an older git would ignore it silently. `cpd doctor` could check the version; not proposed (YAGNI) unless a machine has one.

**Tests** (each fails today for the stated reason, or pins what must not regress)

- **Two Plans, two PRs, one repo, at once** (`no_repo_lock.rs`): two Attempts of different Plans in one repo reach A8 together, and each merge's git is slowed (a `git` wrapper first on `PATH` that sleeps on `update-ref`). Both end `merged` on their own Lead Branch, the two merges' time windows overlap (timestamps in the wrapper's log), and no `error` Event is appended. Fails today: the second waits for the first (no overlap).
- **A Gate of PR A while Jobs merge into PR A:** `pre-merge` = record `git rev-parse HEAD` and the file list, sleep, record again. During the sleep one Attempt of PR A merges. The Gate's records are equal and equal G; the Attempt is `merged` before the Gate ends; the remote holds G after `pr push`, then the merge's commit after the next push; the Lead worktree ends at the Lead tip (rule S). Fails today: the merge waits for the Gate.
- **The clean-up during a Gate (hs-n124):** the PR is marked `merged` while `pr push`'s Gate sleeps; one poll; the Lead worktree is removed, the gate worktree is not, and `pr push` returns `illegal_transition` without pushing.
- **Guard U:** a driver killed right after its CAS (the existing `die_after_step_8` helper, `merge.rs:365`), then `pr rebase`: the Attempt ends `merged` (R ran first), not `crashed`. And a live driver paused after its CAS: `pr rebase` waits, then succeeds once step 10 commits.
- **Rule S:** two merges of one PR whose syncs run in the reverse order end with the Lead worktree at the tip and clean; a staged edit in the Lead worktree is kept and reported `lead_worktree_stale`, never overwritten; `index.lock` held for 1 s then released: S finishes.
- **Rule P:** merge A pushes after merge B pushed its descendant: no `push_failed`.
- **G1:** 8 PR starts in one repo in parallel (`pr_start.rs`), all succeed and `.git/config` gains no `branch.*` entry. Fails today (run 5).
- **G2:** a held `refs/remotes/origin/<base>.lock`, released after 1 s: `pr rebase`'s fetch succeeds. Fails today (run 3).
- **R-G:** two `pr rebase` of one PR at once: one rebases, both exit 0 or one `cas_lost`, and the remote equals the local tip.
- **Kept:** every existing test of `merge.rs`, `pr.rs`, `pr_push.rs`, `pr_cleanup.rs` that does not name the lock, `dirty_target` at the merge, or `require_clean`, passes unchanged.
