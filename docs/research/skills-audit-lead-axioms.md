# Skills audit: cpd-lead against the axioms

## Coverage

- Baseline: `a954761dfe6e7b6316f955ada638e363c170af55` (`git merge-base HEAD origin/main`).
- Read in full: `pi/skills/cpd-lead/SKILL.md` lines 1-451 (the Spec says 452; `wc -l` gives 451; read in four chunks); `pi/skills/cpd-lead/references/spec-files.md` lines 1-161; `pi/skills/cpd-lead/references/questions.md` lines 1-58; `pi/skills/cpd-lead/references/pr-description.md` lines 1-104; the Findings of `docs/research/axioms-audit-agents.md` about cpd-lead (lines 13-20).
- Checked for enforcement before judging prose rules: `pi/lib/*.ts` by `grep` (`noblock.ts` 30 s bound, `secretguard.ts` `op`, `mainguard.ts`, `askguard.ts` limit 1500, `loop-guard.ts`); `pi/skills/skill.test.ts` test titles (lines 25-575).
- Not read: other files; no web; no `cpd` command beyond read-only ones (none was needed).

## Findings

| ID | Lens | Where | Kind | Quote | Evidence | Proposed fix |
|---|---|---|---|---|---|---|
| LA-1 | 4 | pi/skills/cpd-lead/SKILL.md:231 | wrong | `(see [Orchestrating](#orchestrating) step 6)` | Explicit over implicit: SKILL.md:272 is step 6 `Lock`, and the legal forms of a draft and of decided work are step 7 (SKILL.md:281, :288); same wrong step at :319 and :441; `(step 5)` at :231 is `Review` (:260) | Point the three links to step 7 and drop `(step 5)` |
| LA-2 | 4 | pi/skills/cpd-lead/SKILL.md:208 | conflict | `you add it yourself, as a commit in the Lead worktree of a Workflow of that repo` | A primary checkout is a mirror of origin/main / Explicit over implicit: SKILL.md:40 says a repo's configuration is a policy file outside the repo and `Never ... commit product changes`; :208 says commit it in the Lead worktree and, one sentence later, write it at `error.details.file` | Say once that the policy file is written outside the repo and nothing is committed |
| LA-3 | 4 | pi/skills/cpd-lead/SKILL.md:281 | conflict | `it needs (`--needs`) a `queued` or `running` Job;` | Explicit over implicit: SKILL.md:58 says a draft is legal only as a stub or under review, :290 and docs/commands.md:205 refuse a complete draft needing a queued or running Job (`draft_parked`); :281 names that need as a legal form of any draft | Add `as a stub` to that form, or cite the Rules line instead of restating |
| LA-4 | 4 | pi/skills/cpd-lead/references/spec-files.md:99 | conflict | `goes to a Worker as an ordinary Job.` | Explicit over implicit / DRY: SKILL.md:138 and :142 send grounding that needs more than reading to an explore Job (`--kind scout\|research`, no Workflow, no draft); spec-files.md:99-105 describes a second, older route (a research Job with `touches` and follow-up drafts) | Delete `## Research Jobs` or mark it as the docs-Workflow research Job of SKILL.md:306 only |
| LA-5 | 4 | pi/skills/cpd-lead/references/pr-description.md:97 | conflict | `registered in backlog. Metrics and cache hit rate dashboard are future work; tracked separately.` | No planned work only in chat: pr-description.md:26 and :41 require follow-ups to name registered Job and Workflow keys; the Good example names none (`backlog`, `tracked separately`) | Rewrite the example with `see Workflow cache-batch` style keys |
| LA-6 | 4 | pi/skills/cpd-lead/SKILL.md:258 | bloat | `Then renumber the later steps and their references inside the file.` | Delete first, but check the fence: an editing instruction left in the text; there is nothing to renumber (steps 1-6 run on at :256-:272, `rg -n 'renumber' pi/skills/cpd-lead` finds only this line) | Delete the sentence |
| LA-7 | 4 | pi/skills/cpd-lead/SKILL.md:441 | bloat | `Register every further fix, batch or follow-up Workflow you intend as soon as you plan it` | DRY: items 3-5 at :439-:441 are numbered after item 6 (:438), and each restates a rule stated elsewhere (:58; the `plan-done` row :310; :440 repeats :310 `Every Workflow is merged`) | Renumber the list and delete :440-:441 (keep :439 only if `plan-done` does not say it) |
| LA-8 | 4 | pi/skills/cpd-lead/SKILL.md:58 | bloat | `A follow-up Workflow is registered the same way` | DRY / KISS: one 2,400-character line; the `--base`/`--needs`/`--no-needs` rule and `needs_required` appear twice in it and again at :242 and :52; the draft forms again at :281 and :290 | Cut line 58 to the rule and link to Scoping and Orchestrating for the commands |
| LA-9 | 4 | pi/skills/cpd-lead/SKILL.md:252 | bloat | `order across Workflows is stacking, `--base <workflow-key>`, the default for a same-repo Workflow` | DRY: stacking versus `--needs` is stated at :183, :202, :242, :252 and :408 (five times); spec-files.md:102 a sixth | Keep the one at :202 (`The order between Workflows`) and link the rest |
| LA-10 | 4 | pi/skills/cpd-lead/SKILL.md:351 | bloat | `A `gate_failed` Attempt was already retried once at the same Tier` | DRY: the same rule, with the same words, is in the `gate_failed` row at :343 (`pressing retry unchanged repeats the failure`) and in the bullet at :351 | Delete the bullet at :351 |
| LA-11 | 4 | pi/skills/cpd-lead/SKILL.md:398 | bloat | `never cancel it (a cancel stands: commands.md §5.3)` | DRY: the failed open/update Job rule is at :307, :354, :372, :398 and :400 with the same `never cancel` and repair commands | Keep :354 and link the others to Failed Jobs |
| LA-12 | 4 | pi/skills/cpd-lead/SKILL.md:158 | bloat | `Group independent questions into one round` | DRY: the round rules (2-4 options, `Recommended:` first, group independent questions, any phase) are at SKILL.md:41, :158, questions.md:6, :9 and :11 | Keep questions.md and link from :41 and :158 |
| LA-13 | 4 | pi/skills/cpd-lead/SKILL.md:151 | bloat | `read that ceiling from `cpd tiers` and the config` | DRY: the Tier range and `no_submit` ladder text is at :151, :227, spec-files.md:60, :64 and :159; the explore Dependencies/Worker/Refusals bullets repeat at :150, :153 and spec-files.md:158-:161 | Keep the Tier rules in spec-files.md and the explore rules in Exploring; link the other |
| LA-14 | 4 | pi/skills/cpd-lead/references/spec-files.md:28 | bloat | `Never a full unfiltered run (the whole workspace, `npm test` of everything)` | DRY: SKILL.md:228 states it, spec-files.md:28 and :48 state it again; size `about 150 turns` is at SKILL.md:225 and spec-files.md:38; repo-relative paths at SKILL.md:230, spec-files.md:26 and :27 | Keep spec-files.md and link from SKILL.md |
| LA-15 | 4 | pi/skills/cpd-lead/references/spec-files.md:60 | bloat | `pro-d328 to pro-d332, which supersede the fixed `H0`–`H3` and `S0`–`S3` of pro-d82` | Recency wins: the superseded decisions pro-d82 and pro-d84 leave force; the live text still names them and their old values | Delete the supersession clause |
| LA-16 | 4 | pi/skills/cpd-lead/references/spec-files.md:86 | bloat | `(formerly `[tests]` before the policy file migration)` | Delete first, but check the fence: migration history in a live reference; SKILL.md:208 and spec-files.md:89 use `[test-commands]` only | Delete the parenthesis |
| LA-17 | 4 | pi/skills/cpd-lead/SKILL.md:48 | bloat | `fix the cause and rerun. Never work around a refusal.` | prior: SKILL.md:37 (axioms-audit-agents.md:13), Fix the cause, not the symptom: the clause is still at :48 and again at :106 | Replace with a pointer to the axiom |
| LA-18 | 4 | pi/skills/cpd-lead/SKILL.md:56 | bloat | `Ask the User only for decisions you cannot derive.` | prior: SKILL.md:39 (axioms-audit-agents.md:14), Ask, don't assume: still at :56, with `Resolve first, ask second` before it | Delete the sentence |
| LA-19 | 4 | pi/skills/cpd-lead/SKILL.md:58 | bloat | `No planned work lives only in your chat.` | prior: SKILL.md:41 (axioms-audit-agents.md:15), No planned work only in chat: still at :58 (title and body are the axiom) | Replace with a pointer to the axiom |
| LA-20 | 4 | pi/skills/cpd-lead/SKILL.md:59 | bloat | `it needs no Plan review` | prior: SKILL.md:42 (axioms-audit-agents.md:16), Decided work skips review: still at :59 | Replace the clause with a pointer to the axiom |
| LA-21 | 4 | pi/skills/cpd-lead/SKILL.md:229 | bloat | `a fresh Worker with only this Spec must finish without asking` | prior: SKILL.md:144 (axioms-audit-agents.md:17), Ask, don't assume: still at :229 | Delete the clause |
| LA-22 | 4 | pi/skills/cpd-lead/SKILL.md:243 | bloat | `A bug or flake fix starts with a test that reproduces it (reproduce first).` | prior: SKILL.md:158 (axioms-audit-agents.md:18), No flakes, no leaks, no orphans, no sleep, no errors: still at :243 | Replace with a pointer to the axiom |
| LA-23 | 4 | pi/skills/cpd-lead/SKILL.md:288 | bloat | `It shows in the Plan view and runs without a second approval.` | prior: SKILL.md:200 (axioms-audit-agents.md:19), Decided work skips review: still at :288 | Replace with a pointer to the axiom |
| LA-24 | 4 | pi/skills/cpd-lead/SKILL.md:319 | bloat | `needs no review (when unsure, a draft)` | prior: SKILL.md:225 (axioms-audit-agents.md:20), Decided work skips review: still at :319 (and :59, :288) | Replace with a pointer to the axiom |
| LA-25 | 4 | pi/skills/cpd-lead/references/pr-description.md:35 | style | `in these two questions answered` | Brevity is not vagueness: the sentence is garbled, and :35 and :37 restate the template placeholders of :14 and :18 | Rewrite as `answer: what is broken, and what this PR does about it` or delete |
| LA-26 | 4 | pi/skills/cpd-lead/references/questions.md:3 | style | `(trial t5)` | Explicit over implicit: `t5` is an opaque reference with no file or record to read | Name the record or drop the parenthesis |

## Prior audit status

Prior findings about `cpd-lead` in `docs/research/axioms-audit-agents.md` (old line, then the current line):

- axioms-audit-agents.md:13 (SKILL.md:37, Never work around a refusal): still open, SKILL.md:48.
- axioms-audit-agents.md:14 (SKILL.md:39, Ask the User only for decisions you cannot derive): still open, SKILL.md:56.
- axioms-audit-agents.md:15 (SKILL.md:41, No planned work lives only in your chat): still open, SKILL.md:58.
- axioms-audit-agents.md:16 (SKILL.md:42, it needs no Plan review): still open, SKILL.md:59.
- axioms-audit-agents.md:17 (SKILL.md:144, Complete and guess-free): still open, SKILL.md:229.
- axioms-audit-agents.md:18 (SKILL.md:158, reproduce first): still open, SKILL.md:243.
- axioms-audit-agents.md:19 (SKILL.md:200, runs without a second approval): still open, SKILL.md:288.
- axioms-audit-agents.md:20 (SKILL.md:225, needs no review (when unsure, a draft)): still open, SKILL.md:319.
- axioms-audit-agents.md:21-29 (cpd-manager, worker.md, worker.rs rows): not applicable, not about cpd-lead.

## Considered, not reported

- Bug report protocol step 2, `Apply the mitigation ASAP`" (SKILL.md:52) against Fix the cause, not the symptom: not reported; step 3 queues the root-cause Job, so the symptom patch is not left alone.
- `Never run gh run rerun`, `gh pr merge --auto`, Slack tools (SKILL.md:42, :418, :425): no guard in `pi/lib/*.ts`; Guards only for proven bad behaviour forbids adding one without evidence, so the prose stays.
- `Mark a Job urgent only when the User explicitly asks` (SKILL.md:300): whether the User asked cannot be checked by a script, so it cannot be enforced.
- `Never run cpd plan decide, cpd watch ...` (SKILL.md:44): enforced by the adapter (`agent_forbidden`, core/src/error.rs:321), so Prose is not enforcement does not apply.
- `dirty_target` row (SKILL.md:347): an old Attempt's row can still show it (docs/contract.md:483), so the fence exists.
- `2.4% of such pairs conflicted` (SKILL.md:258): measured in `docs/research/plan-lint-measurements.md` and `docs/commands.md`, not a bare claim (file hit by `grep -rn "2\.4%"`).
- Command, flag, code and number existence: proved by `pi/skills/skill.test.ts` (tests at lines 92 and 483), so out of scope.
