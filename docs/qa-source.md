# QA toolkit source and CPD consumer

Canonical source: https://github.com/clayton-duarte/skills , package `skills/repo-qa`.
Migration tracking: [#1152](https://github.com/clayton-duarte/cpd/issues/1152).

CPD's `tools/repo-qa` is a pinned, unmodified consumer snapshot, not another
source of truth. [tools/repo-qa.upstream.json](../tools/repo-qa.upstream.json) records
the immutable upstream commit, package path and SHA-256 of every package file.
The reset-pacing source is merged in
[skills #9](https://github.com/clayton-duarte/skills/pull/9); the manifest pins its
merge commit. The entire package matches that merged tree byte-for-byte.
The source dependency is satisfied; CPD still requires User review and merge.
CPD tests reject changed, added, removed or
non-file entries that differ from that snapshot.

The User asked for a standalone source repository. Keeping a pinned copy preserves
ordinary CPD checkout, offline verification and CI without configuring access to
a second private repo. A submodule or package publication would add checkout/token
or release machinery; none is needed for this small dependency-free skill.

CPD continues to own `qa.config.json`, this project profile/decisions, pilot records
and `scripts/qa.mjs` compatibility entry. Existing attribution/batch markers and
attribution remains. The User replaced cardinal caps with the live weekly gate in #1187. The related skills repo is included in the same allowance,
so this source migration does not manufacture a fresh budget. CPD is still the
product priority; unrelated future consumers establish explicit owner-approved
budget groups. One serialized operator owns a shared group.

## Updating the snapshot

1. Make/research the generic skill change in `clayton-duarte/skills` on a branch,
   run its tests, and get the User's source PR review/merge. Do not fix it only in
   CPD's vendored copy. Project-specific settings belong in CPD's own profile/config.
2. In a CPD branch/worktree, replace the complete `tools/repo-qa` directory with
   the `skills/repo-qa` tree of that exact upstream commit. Use a local source
   checkout or a Git archive of the pinned commit; no runtime network bootstrap.
   Do not leave deleted upstream files behind or copy the source repo's `.git`.
3. Regenerate `tools/repo-qa.upstream.json` from those source files (relative path
   to SHA-256), recording the exact repository/40-character commit/package path.
   Keep metadata outside the package so the package remains byte-for-byte equal
   to its canonical source. Record the source PR in the consumer PR.
4. Run `node --test scripts/qa.test.mjs`, applicable syntax/portability checks and
   `git diff --check`; open the consumer PR for User review. The integrity test
    validates the snapshot and the existing suites validate behavior/compatibility.

The snapshot test checks files against the committed hash manifest **offline**;
it does not fetch upstream or prove the manifest's provenance by itself. During
each update, independently compare every file with the named source commit
(including file names and deletions), rather than merely hashing local edits.
Never update the vendored implementation independently of source. Upstream
merges do not silently change CPD: adoption is an explicit reviewed pin update.

## Synchronization options reviewed (2026-11-16)

There is one maintained implementation in skills. The current vendored snapshot
does duplicate file bytes in CPD's Git history; it is not a second editable fork.
Do not describe this as zero duplication or automatic synchronization.

| Option | What CPD records | Checkout / CI consequences |
| --- | --- | --- |
| Current pinned snapshot | Package files plus immutable source revision/hashes | Ordinary checkout and offline tests; no second repository credentials. |
| Git submodule | A commit pointer plus `.gitmodules`, not package files | Initialize/update the submodule in each clone/worktree and opt in to CI submodule checkout; private source access is required. |
| Git subtree | Imported files and source history/provenance | Ordinary checkout works, but files are still duplicated; it does not meet a zero-tracked-copy goal. |

**Recommendation for this PR:** retain the existing verified snapshot to keep
the pacing policy review independent of checkout/authentication changes.
Submodules are a valid follow-up if avoiding tracked copies is worth the extra
bootstrap requirements; they do not automatically track upstream merges.
No automatic updater, scheduler, credential or CI permission is added here.

A submodule migration would mount the **whole** skills repository (for example
at `tools/skills`), then import the toolkit from
`tools/skills/skills/repo-qa`. Gitlinks pin repository commits, not a repository's
subdirectory. Update the CPD compatibility imports, tests and documentation
together; remove the old vendored package/manifest rather than retaining both.
Keep CPD's config/profile outside the submodule. Changes still happen upstream
first; CPD adopts a reviewed commit pointer, not a floating `main`.

Before migration, validate:

- Fresh clones and new worktrees using `git submodule update --init --recursive`,
  with a clear failure message when the dependency is missing.
- Both workflows (`.github/workflows/ci.yml` and `e2e.yml`), which currently do
  not request submodule checkout. The default repository-scoped `GITHUB_TOKEN`
  does not supply access to another private repository. Provision a least-
  privilege read credential separately; do not assume a developer's SSH access
  exists on runners or expose credentials to untrusted PR code.
- An exact approved gitlink revision, clean submodule working tree, offline
  tests after initialization, source-before-consumer updates and rollback to a
  prior pinned revision. Submodule updates must remain reviewed CPD commits.

Sources consulted 2026-11-16:
[Git submodules](https://git-scm.com/docs/gitsubmodules) documents commit gitlinks,
explicit initialization and update semantics;
[actions/checkout](https://github.com/actions/checkout#checkout-multiple-repos-private)
documents private secondary-repository authentication and opt-in submodules.
The current workflows pin checkout v7.0.1 and grant only `contents: read`.

For this initial migration, merge the skills source PR before the CPD consumer
PR. The shared tracking issue closes only after both complete; neither partial
PR alone means the migration is finished. No agent merges autonomously.

## New agents and repositories

Tell a new agent to clone `clayton-duarte/skills` and read
`skills/repo-qa/SKILL.md` from that checkout. Copy the entire skill package or use
the agent's supported skill-install location, record its source commit, and follow
its adoption guide. Config/profile/permissions must be grounded in the new target
repo. Do not copy CPD machine data, active run IDs or an assumed allowance.


Skills #8 supplied the earlier daily policy; skills #9 replaces CPD's selection
with continuous reset pacing. User review/merge of CPD remains required, and no
new QA run is authorized by the pin update.
