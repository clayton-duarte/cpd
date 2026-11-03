# Skills audit: cpd-manager drift (lens 2) and Manager finding M7

## Coverage

- Baseline commit (`git merge-base HEAD origin/main`): `a954761dfe6e7b6316f955ada638e363c170af55`.
- Read in full: `pi/skills/cpd-manager/SKILL.md`, lines 1-173.
- Sources checked: `docs/commands.md` (line 106 key syntax, line 143 `invalid_session_id`, line 3005 `broadcast`, section 15.24 at line 5158), `pi/lib/updates.ts:35`, `pi/lib/askguard.ts:4`, `pi/lib/follower.ts:14`, and live `--help` of `session send`, `session open`, `plan new`, `job retry` and `context pending`.
- Not read: other skills and files, as the Spec names only this one. `cpd session send` was never run.
- Result: the skill has no drift finding of its own. The only row is M7, which the baseline does not reproduce.

## Findings

| ID | Lens | Where | Kind | Quote | Evidence | Proposed fix |
|---|---|---|---|---|---|---|
| DM-1 | 2 | pi/skills/cpd-manager/SKILL.md:159 | wrong | `cpd session send <context>:<plan> "<text>" --unblocks "<reason>"` | M7 is refuted at the baseline, so this row records the claim and not a defect. `git log --oneline origin/main -- core/src/session/resolve.rs` shows `85337fc6f Resolve Sessions by agent key... (#790)`, an ancestor of HEAD. `cpd session show skills-audit:skills-audit-lead` returned `"ok":true`. `session send` was not run: unverified. | None needed. Drop M7 from the final report as a defect, or report it as already fixed by #790. |

## Manager finding M7

**Verdict: refuted at the baseline (`a954761df`).** The skill's `<context>:<plan>` form is the documented key syntax (`docs/commands.md:106`, section 15.24). The resolver that section 15.24 calls another Plan's work has landed.

- `git fetch origin`, then `git log --oneline origin/main -- core/src/session/resolve.rs`: `85337fc6f Resolve Sessions by agent key, relative key and legacy Manager alias (#790)`, plus older `6a513e407` and `070ddee9d`.
- `git merge-base --is-ancestor 85337fc6f HEAD` exits 0, and `core/src/session/resolve.rs` exists in the worktree. So the resolver is in the files audited. The Manager's run probably used an older installed `cpd` binary: unverified.
- Live `timeout 30 cpd session show skills-audit:skills-audit-lead`, exit 0 (truncated to the first 200 characters): `{"ok":true,"open_escalations":[],"session":{"archived_at":null,"asleep_at":null,"attempt":null,"by_user":true,"context":"skills-audit",...`. The same output has `"key":"skills-audit:skills-audit-lead"` and `"id":"9060b897-cb28-4b35-b034-a5be9dafaab2"`.
- `session show` and `session send` both take a Session reference `<id>`. The `session send` help is `Usage: cpd session send <id> (<text> | --file <f>) --unblocks <reason>`. `session send` itself was not run: unverified.
- Per `docs/commands.md:106`, only a value with `:` that is not a 2 or 3 segment key is `invalid_session_id`.
- The Manager's line numbers (48, 59, 132, 142, 152, 157) are stale. `grep -n 'session send <context>:<plan>'` gives 7 lines. Verified list: **50, 62, 134, 136, 145, 159, 160**.
- This repo's change that added the resolver left the skill text as it was. The skill text matches the docs.

## Considered, not reported

- Lead's guess that M7 is a live defect at baseline: refuted. The key resolves via `session show`, and the resolver landed in #790 before the baseline. The Spec asked for a `wrong` row regardless, so DM-1 is that row with a refuting Evidence cell.
- Line 62 "`--unblocks` is always required": the help shows `--unblocks <reason>` with no default. Correct.
- Line 62 `broadcast` 60-minute rule: `docs/commands.md:3005` says 60 minutes, refusal `broadcast`. Correct.
- Line 71 "G2 (the 1500-character limit)": `pi/lib/askguard.ts:4` has `LIMIT = 1500`. Correct.
- Line 71 "longer than 30 seconds" bash limit: not checked in the adapter source. Unverified, not reported.
- Line 143 "at most 20 lines": `pi/lib/updates.ts:35` has `MAX_LISTED = 20`. Correct.
- Line 62 `job retry ... --tier <T>`: the help marks `--tier` required. Correct.
- Line 36 `plan new` flags `--title` and `--brief-file`: both exist, and `--jira` exists. Correct. Other flags exist too (`--tier`, `--runner`), but the skill says only that none takes a brief text.
- Line 153 phases grounding, plan, dispatch, deploy, done: `core/src/driver/start.rs:543` lists the first four. `done` was not checked: unverified.
