# Glossary applied: terms-skill (the Lead and Manager skills)

Line numbers are of the files after the edit (HEAD of this Job). Rows a, b and c are the Spec's rows for the skills (audit `## Stale and wrong statements`, `glossary-audit-pi.md` `## Fix list`). Every quote is at most 15 words.

## Rows

| Where | Row | Applied or not applied | Old quote → new text | Reason when not applied |
|---|---|---|---|---|
| pi/skills/cpd-lead/SKILL.md:394 | a: "sync Job" (old line 385) | Applied | "never with a sync Job (cancel a queued one)" → "never by adding an update Job yourself (cpd's alone: `kind_reserved`; cancel a queued one)" | |
| pi/skills (all files) | a: "sync Job" anywhere in pi/skills | Applied | `rg -n -i 'sync Job' pi/skills` found it only in SKILL.md:385 and in skill.test.ts (a test title and a regex); both reworded | |
| pi/skills/skill.test.ts:752 | a: the test that pinned the retired word | Applied | "never with a sync (or merge )?Job" → "never by adding an update Job yourself" | |
| pi/skills/cpd-lead/SKILL.md:30 | b: key `pr.gate` (old :30) | Applied | "when the PR's policy says `pr.gate = local`" → "`gate.pr = local`" | |
| pi/skills/cpd-lead/SKILL.md:253 | b: old :244, two uses | Applied | "`pr.gate = local`" → "`gate.pr = local`"; "under `pr.gate = ci`" → "under `gate.pr = none`" | |
| pi/skills/cpd-lead/SKILL.md:351, :355, :357 | b: old :342 (two), :346, :348 | Applied | "`pr.gate = local`" → "`gate.pr = local`"; "Under `pr.gate = ci`" → "Under `gate.pr = none`" | |
| pi/skills/skill.test.ts:720 | b: the guard that pinned the key | Applied | "every 'run(s) the Gate' sentence names pr.gate" → "names gate.pr" | |
| pi/skills/cpd-lead/SKILL.md:3 | c: stage name Deploy in the frontmatter description | Applied | "and Deploy (the open Job that checks and opens each PR" → "and Dispatch (… deploying: the open Job that checks and opens each PR" | |
| pi/skills/cpd-lead/SKILL.md:3 | c: step names in the description | Applied | "dividing into PRs and Jobs, sequencing, getting the Plan approved, dispatching" → "scoping into PRs, splitting into Jobs, orchestrating the Plan review and the lock" | |
| pi/skills/cpd-lead/SKILL.md:18 | c: Deploy as the third stage | Applied | "Context, Plan and Deploy" → "Context, Plan and Dispatch"; no column title named | |
| pi/skills/cpd-lead/SKILL.md:20-24 | c: intro table of stages and steps | Applied | "Grounding, Clarifying, Specifying" … "Mechanical checks, PR, Reviewing and Monitoring, Addressing" → the twelve steps in three rows | |
| pi/skills/cpd-lead/SKILL.md:97-338 | c: the outline | Applied | `## 1. Context`, `## 2. Plan` (Dividing, Sequencing, Approving, Dispatching), `## 3. Deploy` → the Spec's outline (see below) | |
| pi/skills/cpd-lead/SKILL.md:339-349 | c: "3. Deploy" intro | Applied | "Steps: [Mechanical checks], [PR], [Reviewing and Monitoring], [Addressing]" → "Parts: …, [Pull request review], …" | |
| pi/skills/cpd-lead/SKILL.md:374 | c: "Reviewing and Monitoring" (people's review of the pull request) | Applied | "### Reviewing and Monitoring" → "#### Pull request review" | |
| pi/skills/cpd-lead/SKILL.md:348, :362 | c: "Mechanical checks" and "PR" as steps | Applied as subsections | "### Mechanical checks", "### PR" → "#### Mechanical checks", "#### PR", inside Deploying | They are no longer steps. The names stay as the Spec's outline lists them (pro-d143 (2): the Deploy step named PR keeps its name) |
| pi/skills/cpd-lead/SKILL.md:114 | outline: Grilling | Applied | "### Clarifying" (interview, step 2) → "### Grilling"; `cpd plan phase plan` moved here from Specifying (step 2); "once Grounding, Clarifying and Specifying are done" → "once Grounding and Grilling are done" | |
| pi/skills/cpd-lead/SKILL.md:123 | outline: Clarifying | Applied | "### Specifying" (records, assumptions) → "### Clarifying"; "Receive the Requirements. Restate them" moved here (step 1) | |
| pi/skills/cpd-lead/SKILL.md:139 | outline: Scoping | Applied | the rest of the old Clarifying step 1 ("Settle which repos…") and the PR part of "### Dividing" → "### Scoping"; "(see [Dividing](#dividing))" → "(see **The order between PRs** below)", because it would link to itself | |
| pi/skills/cpd-lead/SKILL.md:172 | outline: Splitting | Applied | the Job part of "### Dividing" (from "Write one spec file per Job") and all of "### Sequencing" → "### Splitting"; "return to [Clarifying]" → "return to [Grilling]" (the interview) | |
| pi/skills/cpd-lead/SKILL.md:198 | c: the `approving` step name in prose | Applied | "before `cpd plan lock` in the `approving` step" → "in the Orchestrating step" | no code identifier `approving` exists (rg in core/ and cli/ finds none) |
| pi/skills/cpd-lead/SKILL.md:211 | outline: Orchestrating | Applied | "### Approving" → "### Orchestrating" | |
| pi/skills/cpd-lead/SKILL.md:247-253 | outline: Dispatch stage | Applied | "## 2. Plan … Phases `plan` and `dispatch`" → "## 3. Dispatch … Phases `dispatch` and `deploy`, until `done`" | |
| pi/skills/cpd-lead/SKILL.md:251 | outline: first Dispatch step | Applied | "### Dispatching" → "### Documenting, Red testing, Implementing, Verifying"; the four #### subsections stay under it | |
| pi/skills/cpd-lead/SKILL.md:326 | outline: Reviewing | Applied | "#### Judging material" → "### Reviewing" | |
| pi/skills/cpd-lead/SKILL.md:339 | outline: Deploying | Applied | "## 3. Deploy" → "### Deploying"; Posting for review, Addressing, Review comments are now #### subsections | |
| pi/skills/cpd-lead/SKILL.md (links) | cross-references of moved headings | Applied | `#dividing`, `#sequencing`, `#approving`, `#dispatching`, `#judging-material`, `#reviewing-and-monitoring`, `#3-deploy`, `#clarifying` → the new anchors | |
| pi/skills/cpd-manager/SKILL.md:23 | c: the Manager's flow sentence | Applied | "Context, Plan and Deploy … Clarifying (the interview) and Specifying … Dividing" → "Context, Plan and Dispatch … Grounding and Grilling … Clarifying and Scoping" | The Manager's cut of an Epic into Plans is placed under Scoping (the old Dividing's deliverable cut): my assumption, not stated in CONTEXT.md (unverified) |
| pi/skills/cpd-manager/SKILL.md:23 | c: the Leads' steps in the Manager sentence | Applied | "Dividing it into PRs and Jobs, Sequencing, Approving, Dispatching, and **Deploy** (Mechanical checks, PR, …)" → "Scoping (into PRs), Splitting (into Jobs) and Orchestrating …, and **Dispatch** (Documenting, …, Deploying)" | |
| pi/skills/cpd-lead/references/spec-files.md:74 | links to moved headings | Applied | "(../SKILL.md#dividing)" → "(../SKILL.md#splitting)" (the PR shape is in Splitting) | |
| pi/skills/cpd-lead/references/spec-files.md:102 | links to moved headings | Applied | "([Dividing](../SKILL.md#dividing))" → "([Scoping](../SKILL.md#scoping))" (stacking is in Scoping) | |
| pi/skills/cpd-lead/references/pr-description.md, questions.md | c: stage or step names, "sync Job" | Not applied | | claim false for these files: `rg -n -i 'deploy\|sync\|dividing\|mechanical\|reviewing and'` finds none |
| pi/skills/skill.test.ts:195-231 | test: the flow test | Applied | "## 3. Deploy", "### Dividing", … → the Spec's outline; adds the six #### subsections of Deploying | |
| pi/skills/skill.test.ts:226-231 | test: new, stages and steps | Applied | (none) → one test: the skill's three stage and twelve step headings in order, and CONTEXT.md's stage and step entries | |
| pi/skills/skill.test.ts (section lookups) | test: every `section(…, "### Dividing")` and the others | Applied | "### Dividing", "### Approving", "### PR", "### Mechanical checks", "### Addressing", "#### Judging material", "### Dispatching" → the moved headings; no assertion changed in meaning | Two lookups cover two sections now: the Job-kinds test reads Scoping (shape, `[test-commands]`) and Splitting (`--kind`, `tests`) |
| pi/test/skill-retry.test.ts:31,33 | test outside the Spec's touches | Applied, the Lead allowed it | `section(lead, "### Dispatching")` → `section(lead, "### Documenting, Red testing, Implementing, Verifying")`; `section(lead, "### Mechanical checks")` → `section(lead, "#### Mechanical checks")` | The Lead answered `cpd ask` (escalation terms-skill-a1-q1): the touches list missed it; no assertion changed; its own commit. Both rows it checks (the job-failed Events row, the Mechanical checks bullet) sit under the headings I picked |

## The Spec's outline, as written

```
## 1. Context
### Grounding
### Grilling
## 2. Plan
### Clarifying
### Scoping
### Splitting
### Orchestrating
## 3. Dispatch
### Documenting, Red testing, Implementing, Verifying
#### Starting a later batch / Appending fixes / Failed Jobs / Escalations
### Reviewing
### Deploying
#### Mechanical checks / PR / Pull request review / Posting for review / Addressing / Review comments
```

Check: `rg -n '^#{2,4} ' pi/skills/cpd-lead/SKILL.md` lists them in this order.

## No rule was lost

Twenty key phrases of the rules, chosen across the sections, counted with `rg -c -F` in `git show origin/main:pi/skills/cpd-lead/SKILL.md` (before) and in the file after the edit (lines that contain the phrase):

| Key phrase | before | after |
|---|---|---|
| `Never write product code` | 1 | 1 |
| `Never put a secret value in chat` | 1 | 1 |
| `cpd opens the PRs, you do not` | 1 | 1 |
| `Stay free for the User` | 1 | 1 |
| `Register planned work at once` | 1 | 1 |
| `Decided work is queued, undecided design is a draft` | 1 | 1 |
| `Append-only once locked` | 1 | 1 |
| `until unknowns are minimised` | 1 | 1 |
| `A PR is a substantial deliverable` | 1 | 1 |
| `Adopting a pull request cpd did not open` | 1 | 1 |
| `needs `[test-commands]`` | 1 | 1 |
| `Repo-relative paths only` | 1 | 1 |
| `**PR shape:**` | 1 | 1 |
| `Never lock without approval` | 1 | 1 |
| `Settle the overlaps` | 1 | 1 |
| `Urgent Jobs.` | 1 | 1 |
| `A failed Job keeps all its resources` | 1 | 1 |
| `A red test is wrong` | 1 | 1 |
| `No local review before the PR opens` | 1 | 1 |
| `Never resolve a thread` | 1 | 1 |

Stronger check: a line-multiset diff of the old and new SKILL.md shows only the lines listed in the rows above changed (headings, links, the split of the old Clarifying step 1 and Specifying step list, the Gate key, the sync Job sentence); every other line is identical. The origin/main copy of the Lead skill equals this Job's base copy (`cmp`).

## How the new test fails

Run under `timeout 120`, with `--test-name-pattern 'the Lead skill and CONTEXT.md name the same'`:

- clean tree: pass 1, fail 0;
- SKILL.md heading `### Documenting, Red testing, Implementing, Verifying` changed to `### Documenting, Red testing, Implementing` (Verifying removed): pass 0, fail 1;
- CONTEXT.md entry `**Scoping** (step` changed to `**Scopin** (step`: pass 0, fail 1.

Both files were restored with `git checkout` after each run.

## Gate key: what I checked

`gate.pr` is the key of the full Gate (the `pre-merge` hook, run at an open or update Job's submit and by `pr push`); `gate.merge` is the quick check (`pre-commit`, at every non-review Job merge). Read in core/src/merge/mod.rs:6-8 and :276-285, core/src/pr/push.rs:35 and core/src/policy/default.toml:5-7. The values are `local` and `none` (`enum Gate`, core/src/policy/domain.rs:22); `ci` is only the alias of the deprecated `[pr] gate` key, so "under `pr.gate = ci`" became "under `gate.pr = none`". Disagreement left for the Lead (unverified which text is current): CONTEXT.md's Mechanical checks entry says the Gate runs "when the Workflow's `gate.merge` is `local`", the code and the skill now say `gate.pr`.

## Deferred to the rename

Seen in the skills, not changed here (a command, flag, key, Event kind, code string, cockpit label or test the rename owns):

- `PR` meaning the object (prose: "each PR", "a stacked PR", "adopted PR", "the PR's open Job") throughout both skills and spec-files.md; `<pr-key>` placeholders and "PR keys" in pr-description.md.
- Commands and flags: `cpd pr add|edit|cancel|start|describe` (object commands), `--pr <key>`, `cpd pr open|push|rebase|poll|post|view|reply|merge`, `cpd policy show --pr <key>`, `--jira`, `--adopt`.
- Keys, kinds and codes: `prs` in `cpd plan show`, `pr.jira_required`, `pr.post.enabled`, Events `pr-started`, `pr-feedback`, `pr-opened`, error codes `pr_locked`, `pr_start_failed`, `pr_closed`, `pr_title_required`, the file name `pr-description.md`.
- The phase `deploy` and the words "phase `deploy`" (SKILL.md:8, :249, :341; cpd-manager SKILL.md:112): a stored phase name; the stage is Dispatch, the phase keeps its name (CONTEXT.md "Phase, stage and step").
- The Workflow status `waiting` and `dispatch` as used in "stays `waiting`" and "`dispatch` onwards" (SKILL.md:216, :198 and others): Workflow statuses, qualified in prose by the glossary.
- The sentence "Pull Request" with capitals (SKILL.md:8, "its own Lead Branch and Pull Request"): a pull-request-versus-object wording of the audit's first row, rewritten by the rename.
- The heading `#### PR` (the step named PR keeps its name, pro-d143 (2)) and `#### Mechanical checks` (a mechanism, kept as the Spec's outline lists it).
- pi/skills/skill.test.ts line 110 area (`the term Workflow is retired`): untouched as the Spec says; the later rename rewrites the guard. The test names `all-merged` (cpd-manager "Oversee all-merged PRs"), "cpd opens the PRs", "A PR is a substantial deliverable" and many `cpd pr …` regexes still pin the PR wording.
- cpd-manager SKILL.md:99 "tell the Manager with `cpd session send`" and the "Oversee all-merged PRs" heading, and SKILL.md `#### Addressing` item numbers (6, then 3, 4, 5): audit findings that are not one of rows a to c; not touched (the Spec: only the term fixes).
- "NO DEPLOYS ON FRIDAYS" (SKILL.md:390): a Danger rule on GitHub, not the stage.

## Not verifiable here

- `git diff origin/main --stat` lists 40 files, not the Job's files: this Job's base (f6fe1313, branch glassary/glossary) is not an ancestor of origin/main (merge-base 208af857), so the diff holds the glossary branch's other files. `git diff f6fe1313 --stat` lists only the five files of this Job, the Spec's six (SKILL.md, spec-files.md, the manager SKILL.md, skill.test.ts) plus pi/test/skill-retry.test.ts (allowed) plus this file once it is committed. The skill files are identical in origin/main and in the base.
