# Glossary applied: terms-gloss-2 (CONTEXT.md sections 5 to 11, README.md)

Line numbers are of CONTEXT.md after the edit. Section 11 is "Retired words".

| Where | Row | Applied or not applied | Old quote → new text | Reason when not applied |
|---|---|---|---|---|
| README.md:7 | Fix list README.md:7 (levels table copy) | Applied | "The levels, the cpd objects… \| Epic \| Context" → one sentence pointing to CONTEXT.md | |
| CONTEXT.md:204-208 | Fix list CONTEXT.md:112 (lowercase "Pull Request"); audit "PR (phase)" | Applied | "Pull Requests" → "pull request"; PR entry says abbreviation only | |
| CONTEXT.md:228 | Fix list CONTEXT.md:186 | Applied | "feeds the Dispatch column" → deleted | |
| CONTEXT.md:228,232,236 | Fix list CONTEXT.md:190, 194 (cut to a pointer) | Applied | "Defined once in `docs/contract.md`" → shorter text plus section pointer | |
| CONTEXT.md:236 | Fix list CONTEXT.md:194 (base PR) | Not applied | | PR post's text now omits stacking; Stacked Workflow (gloss-1) owns it |
| CONTEXT.md:298 | Fix list CONTEXT.md:183 ("watcher per workflow") | Applied | "watcher per workflow" → "one watcher per Workflow" | |
| CONTEXT.md:266 | Fix list CONTEXT.md:204, 216 ("PR opened or merged") | Applied | "PR opened or merged" → "pull request opened or merged" | |
| CONTEXT.md:414 | Fix list CONTEXT.md:227 / audit Reviewer | Applied | "Reviewer (removed)" entry → Retired entry | |
| CONTEXT.md:417 | Fix list CONTEXT.md:104 / audit Grounded Lead | Applied | Grounded Lead in section 2 → moved to Retired | the Spec lists it in section 11 |
| CONTEXT.md:331 | Fix list CONTEXT.md:69, 236 (stale `pr.gate`) | Applied | "`pr.gate = "local"`" → "`gate.pr` is `local`" | |
| CONTEXT.md:331 | Fix list CONTEXT.md:236 (PR level) | Applied | "at the PR level" → "on a Workflow's Lead Branch" | |
| CONTEXT.md:331 | Fix list CONTEXT.md:237 (Avoid CI, checks) | Applied | Avoid "CI, checks" → CI; checks (GitHub's word); quick check | |
| CONTEXT.md:331-337 | Fix list CONTEXT.md:240 (Gate vs Quick check) | Applied | Quick check = `pre-commit`; Gate = `pre-merge` | pro-d146; contract §7.3 table still calls `pre-commit` "the Gate" (see below) |
| CONTEXT.md:331 | Fix list CONTEXT.md:236 / audit CI (`.github` absent) | Applied | "the repo's CI is the check" → "where CI exists" (CI entry) | |
| CONTEXT.md:352 | Fix list CONTEXT.md:268 (Domain `pr`, key `pr.gate`) | Applied | "`pr.gate`" example → domain list pointer to contract §8.3 | |
| CONTEXT.md:314 | Fix list CONTEXT.md:292 / audit Slot | Applied | "The number of available concurrent units… one concurrent unit of it" → "One concurrent unit of a Resource" | |
| CONTEXT.md:379,383 | Fix list CONTEXT.md:318, 322 / audit instance data | Applied | "known owners… are `clayton-duarte`…" → points to `cpd policy show` | |
| CONTEXT.md:(Repo policy file) | Fix list CONTEXT.md:277 (blank line) | Applied | two blank lines → one | |
| CONTEXT.md (all Avoid lines) | Fix list CONTEXT.md:109 / D26 | Applied | "_Avoid_: lock" → lock; "as a noun/word for X" on ask, config, install, rule, thread, task | |
| CONTEXT.md:387 | D19 owner | Applied | "_Avoid_: operator, owner" (User) → owner entry: GitHub login only | |
| CONTEXT.md:262 | D20 Tier | Applied | "level, profile" → also "tier (for a colour)"; urgency | |
| CONTEXT.md:272-290 | D22 Session | Applied | "A pi session cpd knows about" → cpd's Session; tmux session, pi session entries | |
| CONTEXT.md:298-306 | engine step, Cockpit columns, Inbox kind `deploy` | Applied | (none) → entries engine step (E1-E17); Cockpit lists columns once | |
| CONTEXT.md:204-240 | Spec section 5 (pull request, draft pull request, pull request review, squash-merged, GitHub Actions workflow) | Applied | (none) → five new entries | |
| CONTEXT.md:339 | Spec: Mechanical checks in section 8 | Applied | in section 4 → moved to section 8; "when `gate.merge` is `local`" → "the Gate" | old key was wrong for open/update Jobs (core/src/merge/mod.rs:281) |
| CONTEXT.md:397 | Spec: φ in section 10 | Applied | in section 4 → moved to section 10 | |
| CONTEXT.md:393 | Spec: Axiom | Applied | "on hs `~/.pi/agent/AGENTS.md`, repo `clayton-duarte/pi-config`" → deleted | instance data |
| CONTEXT.md:28-36 | Spec: Context record, Context store, Distilled Context to a pointer | Not applied | | gloss-1 left a place in section 2; the entries stay there unchanged (the Spec says put them in section 10 only when it left none) |
| CONTEXT.md:401-450 | Spec section 11 | Applied | (none) → 14 Retired entries | |
| CONTEXT.md:414-416 | Sync Job | Applied | audit: "a Job kind that does not exist" → "never a Job kind" | |
| CONTEXT.md:270 | Fix list CONTEXT.md:216 (digest) | Applied | "ends with the **digest**… `Plans:` section" → Plan Events listed in one sentence | |

## Disagreements between the old text, the contract and the code

- **Gate and Quick check.** docs/contract.md:3566 names `pre-commit` "the Gate for `gate.merge`" and :3567 `pre-merge` "the Gate for `gate.pr`"; docs/contract.md:3559 and :3969-3970 and pro-d146 name `pre-commit` the quick check and `pre-merge` the Gate. CONTEXT.md follows pro-d146.
- **When the Gate runs.** contract :3704, :3761 say `pr open` and `pr push` run it when `gate.merge` is `local`; the code runs it under `gate.pr` (core/src/pr/push.rs:35, core/src/pr/mod.rs:407). CONTEXT.md says `gate.pr`, as the code does.
- **Default.** docs/contract.md:3566-3567, :3696 say `ci` is cpd's default; core/src/policy/default.toml:5-7 and contract :4019 say `none`. CONTEXT.md says `none`.
- **Quick check at submit.** contract :3666 says submit runs no quick check; :3620 and core/src/merge/mod.rs:276 run `pre-commit` at the Attempt's merge when `gate.merge` is `local`. CONTEXT.md says "at its merge".
- **Sync Job.** audit says it names no Job kind; the old entry said "retired" nowhere: it was only an Avoid word.

## Deferred to the rename

- docs/contract.md:3559, :3566, :3567: "the Gate" for `pre-commit`; "workflow worktree" (CONTEXT.md retired entry).
- pi/skills/cpd-lead/SKILL.md:385 "sync Job"; pi/skills/skill.test.ts:658.
- The cockpit's third column title in the code (Deploy); `cpd pr add/edit/cancel` and `--pr` for a Workflow; the table `prs`; Event kinds `pr-opened`, `pr-merged`; keys `[deploys] workflows` and `deploy-runs.json` `workflow`.
- Inbox kind `deploy` and domain `deploy` (kept, glossed).
