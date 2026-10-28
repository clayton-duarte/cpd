# Glossary applied: terms-gloss-1 (CONTEXT.md sections 1 to 4)

Line numbers are of CONTEXT.md after the edit. Sections 5 and later of the old file are kept as they were, except the entries that sections 2 to 3 now own (rows "moved").

| Where | Row | Applied or not applied | Old quote → new text | Reason when not applied |
|---|---|---|---|---|
| CONTEXT.md:5 | Fix list CONTEXT.md:3 | Applied | "means what it meant there" → "…, except **Workflow**, which returns with a new meaning" | |
| CONTEXT.md:7-14 | Fix list CONTEXT.md:5 (levels table) | Applied | three rows → four rows with Workflow; "Jira analogy" caption | |
| CONTEXT.md:16 | Spec: transitional note | Applied | (none) → "Transitional note: until the rename lands, …" | |
| CONTEXT.md:18-20 | Spec: one rule, history | Applied | (none) → "PR means only the pull request"; history keeps old words | |
| README.md:7 | Fix list README.md:7 | Not applied | | outside the files this Job may touch |
| CONTEXT.md (Phases) | audit Stale: "PR (phase)" | Applied | "**PR** (phase)" entry → deleted | |
| CONTEXT.md (Phases) | vocab Entries: Grounding (phase), Plan (phase), Dispatch | Applied | three phase entries → one "Phase, stage and step" (146) | |
| CONTEXT.md:146 | Fix list CONTEXT.md:29 (follow-up rule: done Plan returns to dispatch) | Applied | "A PR added later moves it back" → "returns to `dispatch` when a Workflow is added" | |
| CONTEXT.md:26,150,156,206 | Fix list CONTEXT.md:36 (three columns) | Applied | "Context, Plan and Deploy" → "Context, Plan and Dispatch"; no column title named | pro-d145 supersedes the check |
| CONTEXT.md:193 | Fix list CONTEXT.md:66 (step PR) | Applied | step "PR" → Deploying | pro-d145, pro-d146 |
| CONTEXT.md:199 | Fix list CONTEXT.md:69 stale key | Applied | "the PR's `pr.gate` is `local`" → "the Workflow's `gate.merge` is `local`" | |
| CONTEXT.md:196 | Fix list CONTEXT.md:79 | Applied | "no PR Gate" → "no Gate at `gate.pr`" | |
| CONTEXT.md:196 | Fix list CONTEXT.md:69 (`pr open`, `pr push`, `pr rebase`) | Not applied | | command names stay: rename Workflow |
| CONTEXT.md:104 (Grounded Lead) | Fix list CONTEXT.md:104 | Not applied | | Spec says leave it in place (terms-gloss-2 moves it) |
| CONTEXT.md:22 (Distilled Context) | Fix list CONTEXT.md:100 | Not applied | | belongs to terms-gloss-2; kept as is |
| CONTEXT.md:44 | Fix list CONTEXT.md:108 (Plan entity) | Applied | "the term Workflow is retired"; Avoid `workflow` → deleted; "Jira task" → "Jira ticket" | |
| CONTEXT.md:48 | Fix list CONTEXT.md:112 (PR entity) | Applied | "**PR** (entity" → "**Workflow** (entity"; "Distinct from the PR phase" deleted | |
| CONTEXT.md:48 | Fix list CONTEXT.md:112 (lowercase "Pull Request") | Applied | "the Pull Request opened" → "pull request" (also Lead Branch, Open Job) | |
| CONTEXT.md:88 | Fix list CONTEXT.md:120 (Shape) | Applied | (no entry) → **Shape** entry | |
| CONTEXT.md:118 | Fix list CONTEXT.md:152 | Applied | "A PR with a draft never opens by itself" → "A Workflow with a draft Job never opens its pull request by itself" | |
| CONTEXT.md:92 | Fix list CONTEXT.md:194 (Stacked) | Applied | (no entry) → **Stacked Workflow**, names the base Workflow | |
| CONTEXT.md:130 | Fix list CONTEXT.md:248 / D10 (Merged) | Applied | "Avoid: done, completed" → squash-merged named for the pull request | |
| CONTEXT.md:109 | Fix list CONTEXT.md:109 (Avoid lists name nouns) | Applied | "_Avoid_: retry, run" → "retry, run (as nouns)" (Attempt, Ledger, Escalation, Plan) | |
| CONTEXT.md:72 | Entries: Spec Avoid brief/ticket | Applied | "_Avoid_: brief, prompt, ticket" → "brief (that is the Brief), prompt" | |
| CONTEXT.md:72 | Entries: Spec uses `context` | Applied | "context" → "`context`… not the Context entity" | pro-d146 |
| CONTEXT.md:76 | Used but not defined: Worker Spec | Applied | (no entry) → **Worker Spec** (commands.md §16.1) | |
| CONTEXT.md:84 | Used but not defined: needs | Applied | (no entry) → **Workflow need** | |
| CONTEXT.md:100 | Used but not defined: Lead worktree | Applied | (no entry) → **Lead worktree**, Avoid "workflow worktree" | |
| CONTEXT.md:126 | Used but not defined: Ready, held | Applied | (no entry) → **Ready**; waiting Job displays "held" | pro-d146 |
| CONTEXT.md:137 | Spec: Workflow status; D8 | Applied | (no entry) → **Workflow status** | |
| CONTEXT.md:122 | D17/D18 Locked | Applied | "Locked" kept apart from Plan lock and file lock | pro-d146 |
| CONTEXT.md:146 | Used but not defined: Phase, Stage, Step; D21 | Applied | (no entry) → one entry incl. "engine step E7" | |
| CONTEXT.md:150-195 | Spec: new steps | Applied | Specifying, Dividing, Sequencing, Approving, Dispatching, Deploy → 12 step entries | pro-d145, pro-d146 |
| CONTEXT.md:196 | Entries: Reviewing (three review senses) | Applied | "Reviewing (step, with Monitoring)" → Reviewing = Review Job only; "pull request review" named | |
| CONTEXT.md:199 | Spec: Mechanical checks | Applied | step → mechanism, "Not a step: part of Deploying" | |
| CONTEXT.md:202 | Used but not defined: Plan review | Applied | (no entry) → **Plan review**; Avoid "review" alone | |
| CONTEXT.md:112,130,134,140 | Spec: entries owned by sections 2-3 | Applied (moved) | Pending work (Roles), Lead Branch, Merged (Outcomes), Skipped (Policies), Escalation (Roles) → moved up | the Spec lists them in sections 2-3; one source |
| CONTEXT.md:213 onward | Spec: sections 5+ unchanged | Applied | (the rest as in origin/main apart from the moved entries) | |
| CONTEXT.md:207 (φ) | `φ` | Applied | kept in place at the end of section 4 | terms-gloss-2 moves it |
| CONTEXT.md:22-24 | Fix list: "Not a session" | Applied | "Not a session" → "Not a Session" | pro-d146 |
| Monitoring (no entry) | Used but not defined: Monitoring | Applied | heading "Reviewing and Monitoring" gone; polling in Deploying | |

## Disagreements between the old text and the contract

| Contract | Old text | Resolution |
|---|---|---|
| §3.9 stages Context, Plan, Deploy; steps Specifying, Dividing, Sequencing, Approving, Dispatching, Reviewing and Monitoring | the same | superseded by pro-d145 and pro-d146; the glossary uses Dispatch and the new steps (contract not edited: out of my files) |
| §7.3 (line 3560) calls `pre-commit` "the Gate" | Gate = pre-merge, Quick check = pre-commit | the glossary keeps pro-d146: Quick check = pre-commit, Gate = pre-merge; the contract line needs fixing |
| §2.2 "PR" (line 281) defines the object as PR | Workflow | pro-d136 wins; contract still says PR (to the rename) |
| §3.1 phase `dispatch` ends when the last PR opens; "Dispatch" phase text of the old entry ended "when every PR is open" | consistent | recorded in the single phase entry |
| §3.2 does not call Skipped a status; E3 records it (§3.7) | "Skipped" under Policies | defined as a Ready Job E3 skipped, not a stored status |
| §3.9 "Reviewing and Monitoring" lists CI and bot as reviewers | old entry said "CI review" | not repeated; Reviewing is the Review Job only |
| §7.3 "workflow worktree" | Lead worktree | glossary uses Lead worktree; contract wording deferred |

## Deferred to the rename

- Commands and flags in entries: `pr add`, `pr open`, `pr push`, `pr rebase`, `pr poll`, `--pr`, `--needs`, `pr-feedback` Event.
- Tables `prs`, `pr_needs`; the policy key `gate.merge`/`gate.pr` as named today.
- The cockpit's third column title (Deploy today), and contract §3.9, README.md:7 table, docs/ui-preview.md, docs/implementing.md uses of "PR".
- "workflow worktree" in docs/contract.md §7 and code strings (core/src/driver/start.rs:312, unverified line).
- Retired section 11 does not exist until terms-gloss-2: entries point to "see Retired".
