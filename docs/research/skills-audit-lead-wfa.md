# Skills audit: cpd-lead, contradictions, duplication, writing-for-agents fit

## Coverage

- Baseline: `a954761dfe6e7b6316f955ada638e363c170af55` (`git merge-base HEAD origin/main`).
- `pi/skills/cpd-lead/SKILL.md`: lines 1-451 read in full (`wc -l` says 451, the Spec says 452).
- `pi/skills/cpd-lead/references/spec-files.md`: lines 1-161 read in full.
- `pi/skills/cpd-lead/references/questions.md`: lines 1-58 read in full.
- `pi/skills/cpd-lead/references/pr-description.md`: lines 1-104 read in full.
- Rubric read in full first: `pi/skills/writing-for-agents/SKILL.md` (81 lines) and `SKILL-MECHANICS.md` (22 lines).
- Script pass: every `](#anchor)` and `](file#anchor)` link in the four files checked against real headings: 0 broken. Every `step <n>` reference checked against Orchestrating steps 1-7: 6 wrong (LW-1 to LW-6). Numbered lists checked for restarts: one (`#### Addressing`).
- Not read: nothing in the four files. `docs/commands.md` was grepped for `draft_parked` and `gh pr edit` only; no `cpd` command was run.

## Findings

| ID | Lens | Where | Kind | Quote | Evidence | Proposed fix |
|---|---|---|---|---|---|---|
| LW-1 | 1 | pi/skills/cpd-lead/SKILL.md:231 | wrong | (see [Orchestrating](#orchestrating) step 6) | SKILL.md:281 is `7. **Add the later batches**` with the legal forms of a draft; :272 is `6. **Lock.**` | Change to `step 7`. |
| LW-2 | 1 | pi/skills/cpd-lead/SKILL.md:231 | wrong | is added queued (step 5) | Decided-work-queued text is SKILL.md:288 (inside step 7); :260 is `5. **Review.**` | Change to `Orchestrating step 7`, with the link. |
| LW-3 | 1 | pi/skills/cpd-lead/SKILL.md:319 | wrong | [Orchestrating](#orchestrating) step 6, or a follow-up Workflow | Drafts are added in step 7 (SKILL.md:281-286), not step 6 (Lock, :272) | Change to `step 7`. |
| LW-4 | 1 | pi/skills/cpd-lead/SKILL.md:441 | wrong | [Orchestrating](#orchestrating) step 6) | Legal forms of a draft are SKILL.md:281, step 7; step 6 is Lock (:272) | Change to `step 7`. |
| LW-5 | 1 | pi/skills/cpd-lead/SKILL.md:290 | wrong | `cpd job cancel`, as in step 4 | Edit/cancel commands are in step 5, Rejected (SKILL.md:261-266); step 4 is the outline (:259) | Change to `step 5`. |
| LW-6 | 1 | pi/skills/cpd-lead/SKILL.md:321 | wrong | drop one as in [Orchestrating](#orchestrating) step 4) | Dropping a draft is in step 5, Rejected (SKILL.md:261-266); step 4 is the outline (:259) | Change to `step 5`. |
| LW-7 | 1 | pi/skills/cpd-lead/SKILL.md:435 | conflict | and opens only when you do | Rules SKILL.md:45: `cpd opens the pull requests, you do not`; :404 `Never run cpd pr open for a Workflow that is open` | Delete the clause; a PR body is never a Job, cpd opens the PR. |
| LW-8 | 1 | pi/skills/cpd-lead/references/spec-files.md:70 | conflict | and opens only when you do | Same sentence as SKILL.md:435; contradicts SKILL.md:45 | Delete the clause. |
| LW-9 | 1 | pi/skills/cpd-lead/SKILL.md:115 | conflict | `sleep 10; tail -n 20 /tmp/cpd-<plan>/<name>.log` | Rules SKILL.md:57: `Never poll, sleep or run a foreground loop.` | Say `read the log once with tail` and drop `sleep`, or carve the exception out in :57. |
| LW-10 | 1 | pi/skills/cpd-lead/SKILL.md:409 | conflict | run `gh pr edit <n> --base <branch>` right after the pull request opens | Rules SKILL.md:42 allow `gh` for reading; :204 `gh writes you may not make`. docs/commands.md:2276 says `gh pr edit` fails on this repo (title case; --base unverified) | Move the retarget into a cpd command or a Worker Spec, or add it to the :42 allow-list. |
| LW-11 | 1 | pi/skills/cpd-lead/SKILL.md:281 | conflict | it needs (`--needs`) a `queued` or `running` Job; | Same file :290 and :58: a complete draft needing a queued/running Job is `draft_parked`; docs/commands.md:1467 says the legal form is a stub | Write `a stub that needs`; apply the same to the other two forms. |
| LW-12 | 1 | pi/skills/cpd-lead/references/spec-files.md:99 | conflict | goes to a Worker as an ordinary Job. | SKILL.md:142 sends this grounding to an explore Job (`--kind scout\|research`, no `--workflow`); :224 calls both explore | Rename the section `Research Jobs in a docs Workflow` or fold it into Explore Jobs. |
| LW-13 | 1 | pi/skills/cpd-lead/SKILL.md:258 | bloat | Then renumber the later steps and their references inside the file. | Editing instruction left in the text; the steps are numbered 1 to 7 (SKILL.md:256-281) | Delete the sentence. |
| LW-14 | 1 | pi/skills/cpd-lead/SKILL.md:439 | bloat | 3. A Workflow can be added after the lock, in any phase, | `#### Addressing` numbers 1-6 (:433-438) then 3, 4, 5 (:439-441); the three items are not about addressing | Delete the list restart; keep one pointer to `plan-done` (:310). |
| LW-15 | 1 | pi/skills/cpd-lead/SKILL.md:440 | bloat | 4. The Plan is done when every Workflow is merged (`plan-done`). | Same fact in the `plan-done` row, SKILL.md:310 `Every Workflow is merged.` | Delete. |
| LW-16 | 1 | pi/skills/cpd-lead/SKILL.md:441 | bloat | Register every further fix, batch or follow-up Workflow you intend as soon as you plan it | Rules SKILL.md:58 `Register planned work at once` | Delete; point to Rules. |
| LW-17 | 1 | pi/skills/cpd-lead/SKILL.md:59 | bloat | Work that only carries out a decision the User already recorded | Stated again at SKILL.md:58, :231, :288, :306, :319; `grep -n 'only carries out'` hits 58, 59, 231, 288, 306, 319, 441 | Keep one home (Rules :59); other sites say `queued, see Rules`. |
| LW-18 | 1 | pi/skills/cpd-lead/SKILL.md:288 | bloat | **Decided work is not a draft.** | Restates Rules :59 almost word for word | Delete; keep Rules :59. |
| LW-19 | 1 | pi/skills/cpd-lead/SKILL.md:58 | bloat | `draft_parked` (exit 3) — add it queued instead | The same rule is at SKILL.md:281, :290 and :408 (four statements of the legal forms of a draft) | Keep the legal forms once, in Rules :58; others link. |
| LW-20 | 1 | pi/skills/cpd-lead/SKILL.md:418 | bloat | tell the User in one chat line, with the pull request's link, that Workflow `<key>` is ready to post | `ready_to_post` is told at SKILL.md:309, :418 and :424 (three times; `never ask` too) | Keep one home (Posting for review); the table row links. |
| LW-21 | 1 | pi/skills/cpd-lead/SKILL.md:351 | bloat | A `gate_failed` Attempt was already retried once at the same Tier | Same advice as the table row SKILL.md:343 and the open-Job bullet :354 | Delete :351; keep the table row. |
| LW-22 | 1 | pi/skills/cpd-lead/SKILL.md:347 | bloat | Declared, never produced since the User's decision of 2026-10-19 | Row says `Nothing is to be done`: it changes no behaviour | Delete the row. |
| LW-23 | 1 | pi/skills/cpd-lead/SKILL.md:252 | bloat | A Job's Dependencies are Jobs of its own Workflow; order across Workflows is stacking | Stacking vs `--needs` is told at SKILL.md:183, :202, :242, :408; four more times | Keep the Scoping paragraph :202; others link. |
| LW-24 | 1 | pi/skills/cpd-lead/SKILL.md:56 | bloat | never route a Worker's question or a User decision through the Manager | Same as the previous Rule, SKILL.md:55, and :360 | Merge :55 into :56. |
| LW-25 | 1 | pi/skills/cpd-lead/SKILL.md:151 | bloat | A scout is letter `H`, a research Job letter `S` | Same range text at SKILL.md:142 and references/spec-files.md:109, :159 | Keep spec-files.md; SKILL.md links. |
| LW-26 | 1 | pi/skills/cpd-lead/references/spec-files.md:105 | bloat | Promoting an explore finding into the repo | Same at SKILL.md:154 | Delete from one. |
| LW-27 | 1 | pi/skills/cpd-lead/references/spec-files.md:95 | bloat | a `tdd` Workflow that needs `[test-commands]` is refused `tests_unconfigured` | Same at SKILL.md:208 (and :206) | Keep SKILL.md:208; drop here. |
| LW-28 | 1 | pi/skills/cpd-lead/references/spec-files.md:60 | bloat | pro-d328 to pro-d332, which supersede the fixed `H0`–`H3` and `S0`–`S3` of pro-d82 | Superseded decisions kept as history; the line tells the reader to read the range from `cpd tiers` | Delete the pro-d list. |
| LW-29 | 1 | pi/skills/cpd-lead/references/spec-files.md:86 | bloat | (formerly `[tests]` before the policy file migration) | Migration history; the current name is `[test-commands]` | Delete the parenthesis. |
| LW-30 | 3 | pi/skills/cpd-lead/SKILL.md:3 | bloat | Act as the Lead of a cpd 2 Plan (one deliverable, one or more Workflows across one or more repos, inside a Context) | Description is 667 chars (`sed -n 3p ... \| wc -c`) and restates the stage table of :18-24; rubric: Context pointers, `Cut identity the body already carries` | Cut to the trigger branches: Plan start or continue, a cpd Event message; drop the stage list. |
| LW-31 | 3 | pi/skills/cpd-lead/SKILL.md:58 | bloat | A draft is legal only while it is a stub or under review | One Rules bullet of 2403 chars (longest line, `awk` length), holding the draft forms, Workflow needs and E7 text | Move to a `references/registering-work.md` behind one pointer line; leave a two-line rule. |
| LW-32 | 3 | pi/skills/cpd-lead/SKILL.md:206 | bloat | **A Workflow's shape** (`--shape`, stored and enforced | 1659-char paragraph of in-file reference inside the Scoping step; rubric: Progressive disclosure, branching test | Disclose to `references/workflow-shape.md`; Scoping keeps one pointer. |
| LW-33 | 3 | pi/skills/cpd-lead/SKILL.md:331 | bloat | #### Failed Jobs | Failed Jobs (:331-354), Escalations (:356-372) and Steering (:374-380) are reached only on a `job-failed`, `escalation` or steering turn; rubric: Branching is the cleanest disclosure test | Move behind pointers in the Event table rows (see split row below). |
| LW-34 | 3 | pi/skills/cpd-lead/SKILL.md:45 | bloat | **cpd opens the pull requests, you do not.** | SKILL.md has 68 `never` (`grep -c -o -i '\bnever\b'`); rubric: Negation, prompt the positive; this rule is paired with `never cpd job add --kind open` | Lead with the positive (`cpd opens; you read `job-merged`); keep a negation only for `kind_reserved`. |
| LW-35 | 3 | pi/skills/cpd-lead/SKILL.md:296 | bloat | ### Documenting, Red testing, Implementing, Verifying | Proposal, not a fix: keep ONE model-invoked skill (the description must fire on a Plan or an Event, :3); split by disclosure, not by invocation. SKILL-MECHANICS asks for a split only when a distinct leading word triggers or another skill must reach it: none here | Move behind `references/`: Exploring (:140-154), the Scoping detail (:204-212), Failed Jobs, Escalations, Steering (:331-380), Stacked Workflows (:406-412), Posting for review (:420-429), Addressing/Review comments (:431-451), the User-command recipe (:101-121), MCP (:70). Stay as steps: Grounding, Grilling, Clarifying, Splitting, Orchestrating 1-7, the Event table (one line a branch), Rules. |
| LW-36 | 3 | pi/skills/cpd-lead/SKILL.md:309 | style | \| `pr-feedback` \| A Workflow's pull request needs attention: | 1601-char table cell holding a procedure with seven branches; rubric: Information hierarchy, steps buried in reference | Replace the cell with a pointer to Pull request review and Addressing. |
| LW-37 | 3 | pi/skills/cpd-lead/SKILL.md:306 | style | \| `job-merged` \| A Job is Merged. \| Note it. | 1267-char Action cell, five branches in one cell; same rubric | Split per branch into the sections it already links. |
| LW-38 | 3 | pi/skills/cpd-lead/SKILL.md:49 | style | **Skills are for agent discovery; `cpd` commands provide deterministic execution (pro-d204).** | No-op: it does not change a step the Lead takes; rubric: Pruning, no-ops | Delete the bullet. |
| LW-39 | 3 | pi/skills/cpd-lead/SKILL.md:280 | style | where `<body.md` is a file outside the repo | Missing `>` after `<body.md`; the placeholder is `<body.md>` at :272 | Close the placeholder. |
| LW-40 | 3 | pi/skills/cpd-lead/SKILL.md:58 | style | looks like unrelated work; instead, `workflow add` in `dispatch` is refused `needs_required` | Sentence is garbled: the `instead` clause has no object; the correct text is at :242 and :408 | Rewrite or delete; :242 is the source. |
| LW-41 | 3 | pi/skills/cpd-lead/SKILL.md:258 | style | on the ledger of 2026-10-30, 2.4% of such pairs conflicted | A dated statistic with no source in the file; unverified; rubric: relevance, sediment | Delete the sentence. |

## Considered, not reported

- Leads: (a) confirmed, LW-13. (b) confirmed, LW-1 to LW-4 (the forms and `queued` text are in step 7). (c) confirmed, LW-14. (d) confirmed, LW-7 and LW-8.
- Rules :60 `Never delete or reopen` vs :439 `A reopened Plan`: the rule is about Locked Jobs, the later text about a Plan. Not a conflict.
- Rules :42 allows `git merge origin/<base>` in a Lead worktree while `merge` is refused in a primary checkout: scope differs. Not a conflict.
- `questions.md:3` `(trial t5)`: a provenance tag, no behaviour change; left alone.
- `pr-description.md` Bad and Good examples: examples carry weight there and nothing repeats them; kept.
- Split by invocation (SKILL-MECHANICS.md) is not proposed: the description at :3 must fire on a Plan or a cpd Event, and no distinct leading word or other skill needs its own trigger. Disclosure into `references/` is proposed instead (the last Findings row).
- `pro-d` and `commands.md §` tags: cross-references to docs; `pi/skills/skill.test.ts` covers existence of named things, so not audited.
- Cockpit table anchors (:22-24) resolve (script pass).
