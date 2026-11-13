# QA toolkit source and CPD consumer

Canonical source: https://github.com/clayton-duarte/skills , package `skills/repo-qa`.
Migration tracking: [#1152](https://github.com/clayton-duarte/cpd/issues/1152).

CPD's `tools/repo-qa` is a pinned, unmodified consumer snapshot, not another
source of truth. [tools/repo-qa.upstream.json](../tools/repo-qa.upstream.json) records
the immutable upstream commit, package path and SHA-256 of every package file.
The pin is `b62b929017b7bdafded41763b10ec1eb0c1fae95`. CPD tests reject changed, added, removed or
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

For this initial migration, merge the skills source PR before the CPD consumer
PR. The shared tracking issue closes only after both complete; neither partial
PR alone means the migration is finished. No agent merges autonomously.

## New agents and repositories

Tell a new agent to clone `clayton-duarte/skills` and read
`skills/repo-qa/SKILL.md` from that checkout. Copy the entire skill package or use
the agent's supported skill-install location, record its source commit, and follow
its adoption guide. Config/profile/permissions must be grounded in the new target
repo. Do not copy CPD machine data, active run IDs or an assumed allowance.
