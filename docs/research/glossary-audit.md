# Glossary audit: summary for the User

Attempt `aud-terms-a1` of Job `aud-terms`, Plan `glassary`. It consolidates the eighteen part findings files (`docs/research/glossary-audit-<part>.md`). It changes nothing else and decides nothing: the User decides every contested term. Decided and not reopened (Context records pro-d136, pro-d138, 2026-10-28): a Workflow is the object (Plan > Workflows > Jobs); PR means only the pull request; history keeps the old words; no aliases. Sense codes: `object`, `pull request`, `phase`, `step`, `mixed`, `other`. A citation `[X] path:line` names the part (codes below) and the place that part cites.

## Method and coverage

- Parts and codes: V vocab; C1 contract-1; C1b contract-1b; C2 contract-2; C3 contract-3; C4 contract-4; M1 commands-1; M2 commands-2; M3 commands-3; M4 commands-4; M4b commands-4b; P pi; S schema; R core; L cli; T tui; X tests; U usage. All 18 findings files are in the Lead Branch (5349 lines, `wc -l`); I read the Terms, Fix list or Wrong-or-stale, and Counts sections of each, and the Coverage of each.
- Covered in full by the reading parts: `docs/contract.md` 1-4258 (C1 1-748, C1b 749-1205, C2 1206-2419, C3 2420-3490, C4 3491-4258) and `docs/commands.md` 1-5632 (M1 1-1730, M2 1731-2358, M3 2359-3647, M4 3648-4512, M4b 4513-5632); M3 skimmed about 160 lines.
- V read CONTEXT.md, README, BACKLOG, docs/versioning, mark, ui-preview, cockpit-parity in full; docs/implementing.md lines 283-686 only skimmed; docs/interface.md Description column only; ADRs first paragraph only. P read the five skill files and three core texts in full; 56 `pi/lib` files skimmed at hit lines.
- Not read line by line (matched lines only, sampled): S (3505 matches, 110 files; 75 migration bodies not read), R (4605 tokens, 101 of 232 files), L (154 lines, 19 files), T (63 of 170 files), X (6326 occurrences in 207 of 501 files, none read in full). U read the Context records (428 lines) but not the tails of the longest records and not all 290 titles lines.
- Totals are not additive: each part used its own regex. Headline numbers as the parts state them: V 238 PR tokens (79 object, 87 pull request, 51 mixed); C1 335 `PR`; C1b 460; C2 597 tokens; C3 239; C4 444; M1 305; M2 415; M4 126; M4b 115; P 564; R 4605; T 1356; X 6326; S 3505.
- Part-to-part disagreements are shown in `## Contested terms` as "Parts disagree".
- Spot check: I opened the cited place for more than ten rows (listed) and each quote was found: CONTEXT.md:29, :108, :112, :186, :236, :240, :292; docs/contract.md:306, :577, :593, :2296, :3260, :3509, :3557-3560, :3947, :3963; core/src/policy/default.toml:5-7; pi/skills/cpd-lead/SKILL.md:385; docs/commands.md:2026, :3428; tui/src/widgets/legend.rs:198; pi/skills/skill.test.ts:110; docs/versioning.md:5-6. I also ran `rg -n 'PROTOCOL: u32'` (17), `ls core/src/store/migrations/*.sql | wc -l` (75) and `ls .github` (absent).
- One count differs between parts and I did not settle it: "workflow worktree" is 43 uses inside contract §7 for C4, while `rg -c -i 'workflow worktree' docs/contract.md` gives 30 lines for the whole file (lines, not uses; unverified which unit is right).

## The vocabulary

Domains: W work units; P phases, stages and steps; Ro roles; Rt runtime; G git; Po policy; GH GitHub; J Jira; UI cockpit. Verdicts: ok, loose, overloaded, stale, wrong. "Parts" lists the parts that use the term. Each row cites one place.

| Term | Domain | Senses found | Verdict | Parts |
|---|---|---|---|---|
| Workflow | W | S1 the new object; S2 cpd 1 name for Plan (CONTEXT.md:108); S3 GitHub Actions workflow (docs/contract.md:3715); S4 "workflow worktree" = Lead worktree (docs/contract.md:3509) | overloaded | V C1 C1b C2 C3 C4 M1 M2 M3 M4 M4b P S R X U |
| PR | W/GH | S1 the object (CONTEXT.md:112); S2 pull request (CONTEXT.md:185); S3 old phase (CONTEXT.md:29); S4 step (CONTEXT.md:71); S5 a Plan or a Lead's own (ctx:325 in U) | wrong for S1, S3 | all |
| pull request | GH | GitHub's; capitalised "Pull Request" at CONTEXT.md:112 | ok (loose capitals) | V C1 C3 M2 S |
| Plan | W/P | S1 entity; S2 phase `plan`; S3 stage; S4 record kind `plan` (docs/contract.md:685); S5 verb "plan a docs PR" (docs/contract.md:2314) | overloaded | V C1 C1b C2 M1 P |
| Context | W/P | S1 entity (CONTEXT.md:87); S2 stage (:38); S3 record; S4 Spec field `context` (docs/contract.md:403); S5 model's window; S6 pi "Context files" (docs/contract.md:3229) | overloaded | V C1 C1b C3 M1 M4 P |
| Job | W | S1 unit of work; S2 GitHub Actions job (docs/implementing.md:413) | ok (one stale CI use) | V |
| Job kind / Open Job / Update Job | W | work, docs, test, impl, verify, open, update (CONTEXT.md:120) | ok | V C1 C2 |
| sync Job | W | retired word still used (pi/skills/cpd-lead/SKILL.md:385) | stale | P |
| Spec | W | S1 Job's Spec; S2 Spec file (JSON); S3 Worker Spec (rendered); S4 lowercase "spec" (docs/contract.md:2397) | loose | V C1b C2 P M4 |
| Review Job | W | Job whose Worker is a fork of the Lead (CONTEXT.md:135) | overloaded (see Review) | V C1 M1 |
| Dependency / needs | W | Job Dependency vs Workflow need, same flag name (docs/commands.md:1337) | loose | V C1b C2 M1 P |
| Attempt | W | one execution (CONTEXT.md:144); Avoid "retry" vs `job retry` | ok | V |
| Ledger | W | record store (CONTEXT.md:148); Avoid "board" vs message board | ok | V M3 |
| Draft | W/GH | S1 Job not approved (CONTEXT.md:152); S2 draft pull request (CONTEXT.md:124) | overloaded | V C1 C1b C2 C3 C4 M2 M4 P |
| Locked | W | Job state (CONTEXT.md:156) vs `plan lock`, flock (docs/contract.md:172) | overloaded | V C1 C1b C2 C3 C4 M1 M2 |
| Merged / merge | W/G/GH | S1 Job merged into Lead Branch (CONTEXT.md:248); S2 Workflow `merged`; S3 pull request merged (CONTEXT.md:30); S4 merge the base (:124); S5 squash merge (:194) | overloaded | all |
| Ready | W/GH | Ready Job; ready to merge; ready to post; Inbox `pr-ready` (docs/contract.md:343) | overloaded | V C1 C1b C3 C4 M2 M4 P |
| Skipped | W | Ready Job E3 could not start (CONTEXT.md:299); GitHub check conclusion | loose | V C4 |
| waiting | W/Rt | Workflow status; Job display value; Session state; GitHub run status (docs/commands.md:542) | overloaded | V C1 C1b C2 C3 C4 M1 M2 M3 P |
| blocked | Rt | Session state `blocked`; `archive_blocked`; "blocking reason" for an agent (docs/commands.md:3694) | loose (one wrong) | C1 C2 M1 M3 M4 |
| open | W/GH | Job kind, Workflow status, pull request state, Escalation status, `pr open`, `session open` (docs/contract.md:292) | overloaded | all |
| dispatch | P/W | phase; Workflow status; step Dispatching; `job dispatch`; Dispatch column (docs/contract.md:292, :593) | overloaded | V C1 C1b C2 C4 M1 M2 T |
| Shape | W | tdd, docs, config, free; no entry (docs/contract.md:294) | loose (undefined) | V |
| Stacked / base / base PR | W/G | stacked Workflow vs pull request whose base is not default (docs/contract.md:343) | loose | V C1 C1b C2 C3 M1 M2 U |
| adopt / adopted PR | W/GH | a Workflow that took over a pull request (docs/contract.md:308) | loose | C1 C2 M2 P U |
| Lead Branch / Lead worktree | G | per Workflow (CONTEXT.md:244); "workflow worktree" is a second name (docs/contract.md:3509) | stale (second name) | V C1b C4 M1 M2 M3 R X |
| phase / stage / step | P | no entry defines the three (CONTEXT.md:15); step also engine E1-E17, driver A1-A9, algorithm steps (docs/contract.md:1182) | overloaded | V C1 C1b C2 C3 C4 M1 M2 P |
| Grounding | P | phase (CONTEXT.md:18) and step (:41); Grounded Lead | overloaded | V |
| Plan stage / Plan phase | P | stage Dividing..Dispatching (CONTEXT.md:51); phase `plan` | overloaded | V C1b M1 P |
| Deploy | P/Rt/Po/GH | stage (CONTEXT.md:66); phase `deploy`; cockpit column (docs/contract.md:2613); Inbox kind; policy Domains `deploy`, `deploys` | overloaded | V C1 C1b C2 C3 C4 M1 M2 M3 M4 P U |
| Dispatching / Dividing / Sequencing / Approving | P | steps (CONTEXT.md:54-63); Approving also GitHub APPROVE, `secret approve` (docs/commands.md:4515) | ok / overloaded (Approving, Dispatching) | V P M4b |
| Mechanical checks | P | step run by cpd (CONTEXT.md:69) | loose | V C1b P |
| Reviewing, Monitoring | P | step names (CONTEXT.md:75); Monitoring has no entry | overloaded | V P |
| PR (step) | P | step name for opening the pull request (CONTEXT.md:71) | loose (after rename) | V C1b M1 P |
| Specifying | P | step; P says it clashes with Spec (pi/skills/cpd-lead/SKILL.md:119) | loose | V P |
| Review / Plan review | P/W/GH | Plan review; Review Job; pull request review; plannotator review; Inbox `review-requested`; removed Reviewer (docs/contract.md:329) | overloaded | all except L |
| Reviewer | Ro | removed by R19 (CONTEXT.md:227) | stale | V M1 M4 |
| Grounded Lead | Ro | "No longer created (B3)" (CONTEXT.md:104) | stale | V |
| User / Manager / Lead / Worker | Ro | one sense each (CONTEXT.md:201-224); Avoid "owner" under User | ok | V |
| Escalation | Ro | Worker to Lead; Lead to Manager; label "φ: Worker question" (pi/lib/review.ts:16) | loose | V P U |
| Brief | Ro | Context record kind; Manager's brief; Avoid "brief" under Spec (CONTEXT.md:133) | loose | V C3 P M4 |
| Tier | Ro/UI | model Tier; Inbox/dot urgency "tier" (docs/contract.md:577) | overloaded | V C1 C2 C3 C4 M3 M4 U |
| owner | Po/GH | GitHub login; Policy scope; User "owns the ticket"; "a Plan owns a pull request" (docs/contract.md:3707) | overloaded | V C1 C2 C3 C4 M2 M3 M4 P U |
| Epic / Ticket / Task | J | levels table maps Jira words (CONTEXT.md:9); "Jira task" at CONTEXT.md:108 | overloaded | V C1 C1b M1 P |
| Session | Rt | cpd Session (CONTEXT.md:162); tmux session (docs/contract.md:32); pi session | overloaded | V C1 C1b C3 C4 M1 M3 M4 P |
| Event / event / message | Rt | cpd Event (CONTEXT.md:166); pi events; tmux hooks; Avoid "message" (:167) | loose | V C3 M4 |
| Inbox / Inbox item | Rt | one sense (CONTEXT.md:170) | ok | V |
| Inbox kinds `deploy`, `pr-ready`, `review-requested` | Rt | GitHub deployment waiting for approval; pull request ready; team review (docs/commands.md:4515) | overloaded | C2 C3 M3 M4 M4b |
| Cockpit / Deploy column / Dispatch column / Pulse | UI | one column, two names (CONTEXT.md:186 vs docs/contract.md:2613); Pulse undefined | stale | V C3 M3 M4 T U |
| Engine / tick / E-step | Rt | one per machine (CONTEXT.md:182); "engine step E17" vs Grounding step | loose | V C1b M4b |
| PR poll / Feedback item / PR post | GH | pull request words (CONTEXT.md:185-194) | ok (stale "Dispatch column") | V M4b |
| post | Rt/GH | PR post; board post; Slack post; Inbox Post action (docs/commands.md:2225) | loose | V C1 C4 M2 M3 M4 P |
| poll / refresh | Rt | PR poll; "Never poll" vs `cpd pr poll` (pi/skills/cpd-lead/SKILL.md:40) | loose | P M4 |
| Gate | G/Po | pre-merge hook (CONTEXT.md:236); also `pre-commit` "the Gate" (docs/contract.md:3560); "PR Gate" | wrong (contradiction) | V C1 C4 M1 M2 M4b P X |
| Quick check | G | `pre-commit` hook (CONTEXT.md:240) | wrong (contradiction) | V C1 C4 |
| checks / CI | G/GH | Mechanical checks; pull request checks; `watch --check`; Avoid "CI, checks" (CONTEXT.md:237) | overloaded | V C1 C1b C2 C3 C4 M1 M2 M3 M4 P |
| Policy / Domain / Scope / Exposure | Po | Policy (CONTEXT.md:264); Domain `pr` mixes senses (:268); "three scopes" lists four (docs/contract.md:3947) | overloaded | V C4 M2 M4b P S |
| Policy key / Repo policy file / Machine policy / Resolved policy | Po | one sense each | ok | V |
| Known owner / Fallback | Po | instance data in the glossary (CONTEXT.md:318, :322) | stale | V |
| Resource / Slot / Tenant | Rt | Slot defined circularly (CONTEXT.md:292); chat slot (docs/contract.md:787) | wrong (Slot) | V C1b M1 M3 |
| Attach | Rt | MCP attach (CONTEXT.md:310); tmux attach (README.md:22) | overloaded | V C3 |
| MCP server / Axiom / φ / CPD | other | one sense | ok | V |
| digest / Distilled Context | W | Context digest; Plans digest; names a process (CONTEXT.md:100, :216) | overloaded | V |
| record | Rt | Context record; Ledger; Event; history row (CONTEXT.md:92) | loose | V C2 |
| state / status | W/Rt | `status` of Job and Workflow; `state` of Session and pull request (docs/contract.md:328) | loose | V C1 C1b C2 C3 C4 M1 M2 M3 M4 |
| run | Rt/GH | Attempt; GitHub Actions run; `cpd resource run`; `run/` dir (docs/contract.md:604) | loose | V C1 C2 C4 M1 M2 M3 M4 M4b P |
| lock | Rt/G | Locked; `plan lock`; flock; git locks (docs/contract.md:172) | overloaded | all contract and command parts |
| hold / held | Rt | incident hold; draft holds Open Job; "never held" (docs/contract.md:1015) | loose | C1b M1 M2 M4b U |
| parked / asleep / archived | Rt | Session out of view; `asleep` undefined (docs/commands.md:656) | loose | C1b C3 M1 M4b |
| label / reason | Rt | Session state label vs tmux window name (docs/contract.md:2469) | loose | C3 |
| landing / checking / merging | UI | display-only derived states, no entry (docs/contract.md:2743) | loose | C3 |
| workers N/M, W waiting | UI | W counts Ready Jobs but text says "waiting Workers" (docs/contract.md:2740) | wrong | C3 M3 |
| ask / retry / board / config / install / lock | other | Avoid words that are real commands (docs/implementing.md:7, :32, :39, :129) | wrong as Avoid | V P |
| outline / decide / note | other | design outline vs Plan outline; `plan decide` vs `context decide` (docs/commands.md:372) | loose | M1 M4 |
| `pr` policy Domain / `gate.pr` | Po | pull request settings with the Workflow's `jira_required` (docs/commands.md:1756) | overloaded | C4 M2 M4b P S X |
| `pr` JSON key | W/GH | outer key = Workflow, inner `pr` = pull request (docs/commands.md:1794) | overloaded | C1 M1 M2 S |
| `pr_not_ready` | W/GH | two refusals, one code (docs/commands.md:2137, :2251) | wrong | C1 M2 |
| `pr open`, `pr push`, `pr rebase`, `pr view`, `pr describe` | W/GH | act on a Workflow and reach its pull request | mixed | all command parts, S |
| Event kinds `pr-opened`, `pr-merged` | Rt | Workflow status change and pull request fact | mixed | C1b C2 M4 S T |
| `until_pr`, `--until-pr`, `--pr` | W | object keys; `review submit --pr <url>` is a pull request (docs/commands.md:5591) | mixed | C1 M4b S |
| tmux / git / GitHub words used as is | G | `stage`, `history`, `archive`, `queue`, `event`, `hook` | ok | C2 C3 |

## Contested terms

Each subsection is a term whose verdict is not ok. "Parts disagree" shows both readings.

### Workflow (the new object; GitHub Actions workflow; cpd 1's Plan)

- Senses: S1 the object (decided); S2 cpd 1's Plan, retired by D3; S3 GitHub Actions workflow (policy key `[deploys] workflows`, JSON key `workflow`); S4 "workflow worktree" for the Lead worktree.
- Cites: [V] CONTEXT.md:108 (retirement sentence); [C4] docs/contract.md:3509 (S4, 43 uses by C4); [C2] docs/contract.md:2296 (S2).
- Harm: after the rename S2 and S4 read as the new object by accident and the guard `pi/skills/skill.test.ts:110` forbids the word, so it fails once the word returns ([X]).
- Options: a) write S2 as "cpd 1's Plan" and S4 as "Lead worktree"; rename the GitHub identifiers (`actions_workflows`, `actions_workflow`); b) keep S2 as history only and leave S3 identifiers, write "GitHub Actions workflow" in prose; c) a) plus a test that forbids bare `workflow` for S3 and S4.
- Recommended: a). The User decided that a GitHub Actions workflow is written in full and that identifiers are covered; S4 is a second name for one thing (one word per concept).
- Parts disagree: [C1] says rename the key `workflow` of `deploy-runs.json`; [C4] says only the User decides whether the key `workflows` is renamed.

### PR as phase and as step

- Senses: S1 phase `pr`, now `deploy`; S2 step named PR (the pull request is opened).
- Cites: [V] CONTEXT.md:29 (phase entry); [C1] docs/contract.md:593 (`dispatch` or `pr`); [M1] docs/commands.md:9 (step); [X] six stale places incl. cli/tests/engine/doctor.rs:686.
- Harm: S1 is a deleted phase that readers still see in a live glossary entry, in engine/src/doctor/state.rs:92-96 and in test names; S2 is a step whose name is the old object word.
- Options for S2: a) keep "PR" (means the pull request); b) rename the step "Opening"; c) "Opening the pull request". S1: delete the entry and fix the references.
- Recommended: S1 delete. S2 b): the other steps are gerunds (Grounding, Reviewing, Addressing), so "Opening" fits and does not depend on the abbreviation.
- Parts disagree: [V] marks the PR step "ok (after rename)"; [C1b] says the word is right but the table needs prose; [M1] and [P] say rename it.

### Merged (a Job merged into a Lead Branch; a pull request merged on GitHub)

- Senses: S1 Job and Attempt `merged` (fast-forward into the Lead Branch); S2 Workflow status `merged` (its pull request squash-merged); S3 merge the base into the Lead Branch; S4 GitHub squash merge.
- Cites: [V] CONTEXT.md:248; [C2] docs/contract.md:1387; [M4] docs/commands.md:3687 ("job-merged of an open Job": says merged, means the pull request opened).
- Harm: four events behind one word; U records one defect caused by it (ctx:390).
- Options: a) "Merged" only for Job and Workflow status, "squash-merged" for GitHub, "merge the base" for git; b) rename the Workflow status `merged`; c) a) plus a distinct Event for an `open` Job.
- Recommended: a). Cheapest and reversible; the statuses are stored (axiom reversibility). The part disagreement is only about the `job-merged` of an open Job ([M4] wants its own Event; [C3] does not).

### Review in all its uses

- Senses: Plan review (the User approves); Review Job; pull request review (`review_decision`); plannotator review (`review-opened`); Inbox `review-requested` (team); Spec column `jobs.review`; removed Reviewer.
- Cites: [C1] docs/contract.md:329; [C3] docs/contract.md:2441; [M2] docs/commands.md:1781.
- Harm: "pending review" can mean the Plan review or GitHub's; CONTEXT.md:75 says CI reviews a pull request.
- Options: a) three named terms (Plan review, Review Job, pull request review), never bare "review"; b) a) plus rename `jobs.review` to `review_job`; c) leave.
- Recommended: a). Upstream owns "review" for the pull request; one word per concept.

### Open (open Job; status open; `pr open`)

- Senses: Job kind `open`; Workflow status `open`; pull request state OPEN; Escalation status `open`; verb in `pr open`; `session open`; "open work".
- Cites: [C1] docs/contract.md:292; [M1] docs/commands.md:908; [P] pi/skills/cpd-lead/SKILL.md:352 (three senses in one sentence).
- Harm: "the Workflow `open`" sits beside the `open` Job and the `OPEN` pull request.
- Options: a) keep identifiers, write "Open Job", "status `open`", "open pull request"; b) rename the Job kind (`opener`); c) rename the Workflow status.
- Recommended: a). Stored values; prose qualification fixes the harm at no migration cost.

### Draft (draft Job; draft pull request)

- Senses: S1 Job not yet approved; S2 GitHub draft pull request; `merge_state` value `draft`.
- Cites: [V] CONTEXT.md:152 and :124; [C2] docs/contract.md:1386; [M2] docs/commands.md:1773.
- Harm: "never a draft" (CONTEXT.md:124) and "draft Jobs" meet in one entry.
- Options: a) "draft pull request" in full for GitHub's, "Draft Job" for cpd's; b) rename the Job status `draft` to `proposed`.
- Recommended: a). Upstream's meaning wins; the Job status is stored.

### Deploy (stage, phase, GitHub Actions deploy)

- Senses: stage Deploy (starts inside phase `dispatch`); phase `deploy`; cockpit column and pane title; Inbox kind `deploy` (workflow run waiting for approval); policy Domains `deploy` (text) and `deploys` (workflows).
- Cites: [V] CONTEXT.md:66; [C4] docs/contract.md:4003; [P] core/src/policy/default.toml:23 and :53-54.
- Harm: five meanings in one word; the contract itself calls two "unrelated".
- Options: a) keep stage and phase, gloss the other three in prose; b) a) plus rename Domain `deploys` and Inbox kind `deploy`; c) rename the column only.
- Recommended: a) now, b) later if a defect shows. Renaming stored kinds costs a migration for a gloss's benefit (YAGNI).

### Dispatch (phase, step, command, retired column)

- Senses: phase `dispatch`; Workflow status `dispatch`; step Dispatching; command `job dispatch`; "Dispatch column"; the Lead's own step Dispatching that the engine does.
- Cites: [C1] docs/contract.md:292 and :291 (phase and status in one sentence); [V] CONTEXT.md:186; [T] tui/src/widgets/legend.rs:198.
- Harm: a Plan in phase `dispatch` can hold Workflows in status `dispatch`, `waiting` or `open`.
- Options: a) keep phase, step, command; delete "Dispatch column"; qualify the status in prose; b) rename Workflow status `dispatch` (for example `started`); c) rename the phase.
- Recommended: a). The status rename is a one-way door through table CHECK values; do it only if prose fails.

### Context (entity, stage, record, update)

- Senses: entity; stage Grounding-Clarifying-Specifying; Context record; Context store; Distilled Context; Context update; Spec field `context`; "Context files" of pi; the model's context window; cpd 1 phase name.
- Cites: [V] CONTEXT.md:39 and :87; [C3] docs/contract.md:3229; [C1] docs/contract.md:403.
- Harm: the lowercase Spec field differs from the entity by one letter.
- Options: a) keep; write "the Spec's `context` field", "pi context files", "context window"; b) rename the Spec field `background` (touches Spec files and fingerprints); c) rename the stage.
- Recommended: a). The field is stored and in the Plan fingerprint ([M1]; unverified by me).
- Parts disagree: [C1] proposes renaming the field; [M1], [P] only qualify it.

### Gate versus checks versus CI

- Senses: Gate = `pre-merge` (CONTEXT.md:236) but `pre-commit` is "the Gate" for `gate.merge` (docs/contract.md:3560); Quick check = `pre-commit` (CONTEXT.md:240); Mechanical checks (step); pull request checks; `watch --check`; "the repo's CI is the check" while the repo has no `.github`.
- Cites: [V] CONTEXT.md:236; [C4] docs/contract.md:3560; [P] core/src/policy/default.toml:5-7.
- Harm: two documents swap the meaning of Gate and Quick check; CONTEXT.md names the key `pr.gate`, which does not exist: the real keys are `[gate] merge` and `[gate] pr`, both default `"none"` (verified).
- Options: a) CONTEXT.md follows the contract's keys; Quick check = `pre-commit`, Gate = `pre-merge`; fix docs/contract.md:3560; "CI checks" or "pull request checks" for GitHub; b) the contract wins: `pre-commit` is a Gate; c) one name "Gate" with two hooks.
- Recommended: a). Two terms keep the two hooks apart; the contract's keys are what the code reads (default.toml).
- Parts disagree: default `ci`: [C4] docs/contract.md:3560 and [M2] docs/commands.md:1739 say `ci`; docs/contract.md:3963 and default.toml say `none`.

### Epic, Ticket, Task against Jira

- Senses: levels table Epic = Context, Ticket = Plan, Task = Job (CONTEXT.md:9-11); CONTEXT.md:108 says "Jira task" for a Plan; Workflow level has no Jira word; Avoid lists epic, ticket, task.
- Cites: [V] CONTEXT.md:108; [P] pi/skills/cpd-lead/SKILL.md:164 ("one ticket per PR"); [C1] docs/contract.md:267.
- Harm: Plan is "task" in one line and "ticket" in the table; a Jira key sits on the Workflow row.
- Options: a) keep the table; fix outliers to "ticket"; caption it "Jira analogy"; b) drop Jira words from the glossary; c) add a Workflow row mapped to Jira sub-task.
- Recommended: a). Upstream's words kept only as an analogy; no new mapping invented.

### owner

- Senses: GitHub login (Context owner, Policy key); repo owner in `<owner>/<repo>`; Policy scope `owner:`; User "owns the ticket"; "a Plan owns a pull request"; OWNER = newest Plan.
- Cites: [V] CONTEXT.md:201 and :264; [C1] docs/contract.md:614; [C4] docs/contract.md:3707.
- Harm: the Avoid word "owner" under User is a live GitHub term elsewhere.
- Options: a) "owner" = GitHub login only; replace "owns" with "is the pull request of" and OWNER with "owning Plan"; b) leave.
- Recommended: a). Upstream's meaning wins.

### tier

- Senses: model Tier (S2, O3); Inbox and dot urgency "tier" (red, yellow...); "blue tier" migrations.
- Cites: [C1] docs/contract.md:577; [C3] docs/contract.md:2735; [M4] docs/commands.md:4405.
- Harm: "the tier names are the palette's colours" (verified) reads as model names.
- Options: a) Tier = model only; "urgency" for the colour ladder (migration names stay history); b) leave.
- Recommended: a). Same word for two scales on one screen.

### run

- Senses: Attempt run; GitHub Actions run; `cpd resource run`; `cpd session run`; pi agent run; `run/<plan>/`.
- Cites: [C4] docs/contract.md:3715; [M2] docs/commands.md:1830; [M4] docs/commands.md:3729.
- Harm: low; "workflow run" will read as a Workflow run.
- Options: a) leave identifiers; write "GitHub Actions run" in full; b) drop `run` from the Avoid lines; c) rename `attempt run`.
- Recommended: a).

### Plan phase versus stage versus step

- Senses: phase (grounding, plan, dispatch, deploy, done) is the cpd side; stage (Context, Plan, Deploy) is the CPD side; step is the CPD substep; step also names engine items E1-E17, driver A1-A9, algorithm steps and install steps.
- Cites: [V] CONTEXT.md:15; [C1b] docs/contract.md:1182; [C4] docs/contract.md:3601 (labels A8, R4 read as decision ids or Tier letters).
- Harm: no entry defines the words; "stage Deploy" begins inside phase `dispatch`.
- Options: a) one entry "Phase, stage, step"; engine items always "engine step E7"; b) a) plus renumber the merge algorithm labels; c) align stage and phase boundaries.
- Recommended: a) plus b). Pure naming, no stored value.

### Locked versus a lock

- Senses: Locked Job state; `plan lock`; flock files under `locks/`; git locks; Resource Avoid word "lock".
- Cites: [V] CONTEXT.md:156 and :289; [C4] docs/contract.md:3503; [M2] docs/commands.md:2021.
- Harm: low; "Locked context" reads as the Context entity (docs/commands.md:904).
- Options: a) "Locked" (capital) for the Job; "Plan lock" and "file lock" in full; drop "lock" from Resource's Avoid; b) leave.
- Recommended: a).

### Session against tmux and pi

- Senses: cpd Session; tmux session (`cpd`, `cpd-work`); pi session and session file; "Not a session" (CONTEXT.md:88).
- Cites: [V] CONTEXT.md:162; [C3] docs/contract.md:3144 (heading "Sessions" is tmux's); [M1] docs/commands.md:818.
- Harm: "hidden session of the same tmux server" (docs/contract.md:32) names two things.
- Options: a) "Session" = cpd; "tmux session" and "pi session" in full; heading 6.2 renamed; b) leave.
- Recommended: a). Upstream owns "session" in tmux and pi.

### PR in mixed commands, keys and Events (consequence of the rename)

- Senses: commands `pr open`, `pr push`, `pr rebase`, `pr view`, `pr describe`; flag `--pr`; Event kinds `pr-opened`, `pr-merged`; policy Domain `pr`; JSON key `pr`; codes `pr_not_ready`, `pr_adopted`, `pr_closed`.
- Cites: [S] core/src/events/kind.rs:104; [M2] docs/commands.md:1735 (`pr open`); [C4] docs/contract.md:3963 (`pr` Domain).
- Harm: each acts on a Workflow and reaches its pull request; the decided rename does not say which noun wins.
- Options: see D2, D3, D4 and D1 below.
- Recommended: see those decisions.
- Parts disagree: [S] moves `pr view` and `pr describe` to `workflow`; [M2] keeps `pr view` under `pr` and calls `pr describe` a decision.

### Smaller contested terms

- waiting: Workflow status, Job display value, Session state, GitHub run status ([M1] docs/commands.md:542; [C1b] docs/contract.md:842; [C3] docs/contract.md:2687). Harm: a Job has no `waiting` status. Options: a) rename the Job display value `held`, the status-line count `ready`; b) leave. Recommended: a) (W counts Ready Jobs, text says Workers).
- blocked: Session state vs "blocking reason" of kind agent ([M4] docs/commands.md:3694). Options: a) "state reason"; b) leave. Recommended: a), axiom "Blocked means it needs the human".
- Ready: Ready Job, ready to merge, ready to post, `pr-ready` ([C1] docs/contract.md:343; [M4] docs/commands.md:3693; [P] pi/skills/cpd-lead/SKILL.md:366). Options: a) "Ready Job" and "mergeable pull request"; b) leave. Recommended: a).
- base, stacked, adopted PR: Workflow's base, pull request base, `--base` takes a branch or a Workflow key ([C1] docs/contract.md:312; [P] pi/skills/cpd-lead/SKILL.md:149; [U] ctx:410). Options: a) add a Stacked entry, "base Workflow" and "base branch"; b) split the flag. Recommended: a).
- post: PR post, board post, Slack post, Inbox Post ([M3] docs/commands.md:3405; [M4] docs/commands.md:3841). Options: a) qualify always; b) rename `board post`. Recommended: a).
- Event / message: Avoid "message" under Event but Events are delivered as messages ([V] CONTEXT.md:167; [C3] docs/contract.md:2485). Options: a) drop "message" from the Avoid; b) "Event text" for the delivery. Recommended: a).
- Slot: defined as a number and as one unit ([V] CONTEXT.md:292); also "chat slot". Options: a) "one concurrent unit of a Resource" plus a "chat slot" entry; b) leave. Recommended: a).
- Scope: four scopes listed under "three scopes" ([C4] docs/contract.md:3947). Fix, not a decision: four.
- Attach: MCP attach vs tmux attach ([V] README.md:22; [C3] docs/contract.md:3156). Options: a) "attach an MCP server" and "attach the cockpit"; b) leave. Recommended: a).
- digest / Distilled Context: Context digest vs Plans digest; entry names a process ([V] CONTEXT.md:100, :216). Options: a) "Distiller", "Plans digest"; b) leave. Recommended: a).
- Policy / config: Avoid "config" under Policy vs `cpd config` ([V] CONTEXT.md:265; docs/implementing.md:32). Options: a) Avoid only as noun for the Policy. Recommended: a).
- ask, retry, board, config, install, run, lock as Avoid words that are real commands ([V] docs/implementing.md:7, :39, :129). Options: a) the Avoid line says "as a noun"; b) drop the word. Recommended: a).
- Spec / brief: Avoid "brief" under Spec while Brief is an entry ([V] CONTEXT.md:133; [P] pi/skills/cpd-lead/SKILL.md:30). Options: a) drop it; b) keep. Recommended: a).
- Skipped / ready / hold / parked / asleep / claim / Pulse / landing: no entry or loose ([C3] docs/contract.md:2743; [M1] docs/commands.md:656; [U] ctx:196). Options: a) add entries for the ones the cockpit shows (Pulse, landing, asleep); b) drop the words. Recommended: a) for cockpit words, b) for the rest.
- Reviewer, Grounded Lead, sync Job: retired words still present ([V] CONTEXT.md:104, :227; [P] pi/skills/cpd-lead/SKILL.md:385). Options: a) delete from CONTEXT.md (ADRs hold history); b) move to a "Retired" section. Recommended: b), because Avoid lists need them.

## Stale and wrong statements

"verified" means I opened the place. Other rows are as the named part reports and I did not open them.

| Where | What is stale or wrong | The fix |
|---|---|---|
| CONTEXT.md:3 | "every term ... not marked new or changed means what it meant" false for Workflow (per V) | add the Workflow exception |
| CONTEXT.md:108, :107, :109 | "the term Workflow is retired"; Avoid word `workflow` (verified :108) | rewrite; Workflow gets its own entry |
| docs/contract.md:210, :267, :854, :2296, :3260, :2443, :2543; docs/commands.md:4251 | cpd 1's Workflow named bare, reads as the new object (verified :2296, :3260) | "cpd 1's name for Plan" or "Plan" |
| CONTEXT.md:29, :112 | entry "**PR** (phase)" and "Distinct from the PR phase" (verified) | delete; phase is `deploy` |
| docs/contract.md:593; docs/commands.md:3428; engine/src/doctor/state.rs:92-96 [L] | phase `pr` still named (verified :593, :3428) | `dispatch` or `deploy` |
| cli/tests/engine/doctor.rs:686; cli/tests/flow/plan.rs:1731; cli/tests/engine/watch.rs:2203; pi/skills/skill.test.ts:469; cli/tests/pr/pr.rs:3256; cli/tests/tui/tui_read.rs:189 | six stale `pr` phase names or texts [X] | rename or relabel |
| CONTEXT.md:69, :236; pi/skills/cpd-lead/SKILL.md:30, :244, :342; docs/commands.md:1600 | key `pr.gate` does not exist; keys are `gate.merge`, `gate.pr` (verified default.toml:5-7) | use the real keys |
| docs/contract.md:3560; docs/commands.md:1739 | default `ci` for the Gate; default.toml:5-7 and docs/contract.md:3963 say `none` (verified) | one default |
| CONTEXT.md:240 vs docs/contract.md:3560 | Quick check and Gate swap meaning (verified both) | align one side |
| docs/contract.md:3660 | runs "no quick check and no Gate" vs :3553, :3557, :3805 [C4] | reconcile once |
| CONTEXT.md:236; docs/implementing.md:413; BACKLOG.md:6 | CI named; `.github` absent (verified), repo runs no GitHub Actions | "where CI exists"; ask whether the BACKLOG item stays |
| CONTEXT.md:186; docs/commands.md:4341; tui/src/widgets/legend.rs:198; tui/src/read/widgets.rs:1 | "Dispatch column" / `PRs (Dispatch)`; column draws no PRs section (verified CONTEXT.md:186, legend.rs:198) | one name; drop the PRs section legend |
| CONTEXT.md:104, :227 | Grounded Lead, Reviewer retired (per V) | delete or move to Retired |
| CONTEXT.md:292 | Slot defined circularly (verified) | "one concurrent unit of a Resource" |
| CONTEXT.md:318, :322 | instance data (`clayton-duarte`) | point to `cpd policy show` |
| docs/versioning.md:5-6 | "Today it is 13" and "45"; code has 17 and 75 (verified) | state the rule only |
| docs/contract.md:306 | defines the PR object as the cpd record (verified) | "A Workflow maps to one pull request but is not it" |
| docs/contract.md:283 | "No other term names it (hs-d36)." false after the rename [C1] | delete |
| docs/contract.md:3947 | "three scopes", four named (verified) | "four scopes" |
| docs/contract.md:577, :2735; docs/commands.md:4405 | "tier names" for colours (verified :577) | "urgency names" |
| docs/contract.md:751, :1013 | "E1–E15", "E1–E16"; the table ends at E17 [C1b] | "E1-E17" |
| docs/commands.md:4684, :4690, :4717, :4734 | reaction queue called E16; it is E17 [M4b] | E17 |
| docs/contract.md:2740 | W "waiting Workers count"; W counts Ready Jobs (:2687) [C3] | "ready-Jobs count" |
| docs/contract.md:2506, :2512, :2571, :3289, :3352, :3478; :3258 | refs "§6.5" should be §6.6; two headings numbered 6.6 [C3] | renumber, fix refs |
| docs/commands.md:4863 and five more `## 23.`; :4952, :5457, :5594 | duplicate section numbers; refs `§24.0`, `§27`, `§21.9` wrong [M4b] | renumber, fix refs |
| docs/commands.md:3, :115-118, :233, :244-246, :585, :659, :736, :801, :1685 | "§n without a prefix means contract.md" reversed; section refs off by one [M1] | fix refs |
| docs/commands.md:2529, :3314 | `§10.15` should be 10.16; `--wake` does not exist [M3] | fix |
| docs/contract.md:732-735; docs/commands.md:1945-1957, :1976-1983, :2021-2024 | duplicate and contradictory `pr add` / `pr edit` rows [C1, M2] | keep one version each |
| docs/commands.md:2026 | "`cancelled` is the PR status that stands for a closed PR" vs :1836 (verified :2026) | "a Workflow given up" |
| docs/commands.md:1781 | "next `pr poll` cleans up the merged PR's worktrees" vs :1838 [M2] | delete |
| docs/commands.md:3687, :3694, :4311 vs :4465, :4113, :4238 | `job-merged` of an open Job; "blocking reason"; Deploy column says no PRs vs PRs; "a Plan's repo/branch" [M4] | reword per part |
| docs/commands.md:3199, :3419, :3424, :3164, :3066 | merge popup still described; E15 absent from `watch` ticks [M3] | delete or add |
| docs/contract.md:670, :312, :732 | `until_pr` format `<plan>/<key>`; `--base` form disagrees [C1] | one form |
| docs/contract.md:2292, :2389, :1256, :2295, :2404, :2410 | indexes `prs_unique`, `prs_live` do not exist; newest migration stated four ways [C2] | correct |
| docs/contract.md:3787, :4038, :4258, :3859 | "precondition 6" absent; "nine above"; script pointer; distiller Tier default three values [C4] | correct |
| docs/contract.md:3509 and 43 uses in §7; docs/commands.md:1076, :1079, :1580, :1600, :2105-2146, :3079; core/src/driver/start.rs:312; 17 test uses | "workflow worktree" = Lead worktree (verified :3509) | "Lead worktree" |
| docs/cockpit-parity.md:1; BACKLOG.md:16; docs/implementing.md:127, :162, :231 | cpd 1 audit uses Workflow 11 times; "Remote workflows"; test area names [V] | archive or relabel |
| pi/skills/cpd-lead/SKILL.md:385, :386-389, :230; pi/skills/cpd-manager/SKILL.md:99, :100 | "sync Job" (verified :385); list numbering; Manager told "tell the Manager"; "all-merged PRs" [P] | fix |
| pi/skills/skill.test.ts:110 | forbids `workflow`; fails once the word returns (verified) | rewrite the guard |
| docs/interface.md:41; core/src/plan/mod.rs:82 | "Cancel the Plan's Jobs and pull requests"; it cancels Workflows [R, L] | fix the clap help, regenerate |
| core/src/pr/mod.rs:6 | `pull_requests` "written only" by poll; merge.rs:109 writes it too [S] | correct the comment |
| core/src/policy/show.rs:193; docs/contract.md:3984 | `policy show` prints the deprecation warning; contract says not surfaced [S] | align |
| core/src/events/kind.rs:158 | `plan-phase` optional `pr` key never written [S] | delete or leave |
| docs/commands.md:216, :1002; docs/contract.md:2314; docs/commands.md:773 | "plan a docs PR", "A PR's title": object or pull request [M1, C2] | decide per sentence |

## Decisions for the User

D1. Event kinds `pr-opened`, `pr-merged` (stored, PROTOCOL bump): a) `workflow-opened`/`workflow-merged`; b) keep, they report a pull request fact; c) new `pull-request-*`. Recommended b).
D2. CLI noun of `pr open`, `push`, `rebase`, `view`, `describe`: a) all to `workflow` ([S]); b) `open push rebase describe` to `workflow`, `view` stays `pr` ([M2]); c) all stay `pr`. Recommended b).
D3. Flag `--pr <key>` and `review submit --pr <url>`: a) `--workflow` and `--pull-request`; b) `--workflow`, and `--pr` keeps meaning only the pull request. Recommended b).
D4. Policy Domain `pr` (Workflow `jira_required`, pull request `post`, `feedback`): a) keep `[pr]` for the pull request, move `jira_required` to `[workflow]`; b) split into `[workflow]` and `[pull_request]`; c) leave. Recommended a).
D5. The Gate keys in CONTEXT.md: a) use the code's `gate.merge` and `gate.pr`; b) rename the keys by what they gate. Recommended a).
D6. Gate versus Quick check: a) Quick check = `pre-commit`, Gate = `pre-merge`, fix docs/contract.md:3560; b) the contract wins; c) one name. Recommended a).
D7. Deploy step named "PR": a) keep; b) rename "Opening"; c) "Opening the pull request". Recommended b).
D8. Workflow status words `dispatch`, `waiting`: a) keep, qualify in prose; b) rename `dispatch` to `started`; c) rename both. Recommended a).
D9. Cockpit right column: a) Deploy; b) Inbox; c) Host; d) keep two names. Recommended a); delete "Dispatch column".
D10. Merged: a) Job only, "squash-merged" for GitHub; b) rename the Workflow status. Recommended a).
D11. Review: a) three named terms; b) a) plus rename `jobs.review`; c) leave. Recommended a).
D12. Open: a) qualify in prose; b) rename Job kind `open`; c) rename Workflow status `open`. Recommended a).
D13. Draft: a) "draft pull request" in full; b) rename the Job status. Recommended a).
D14. `[deploys] workflows` and JSON key `workflow` of `deploy-runs.json`: a) `actions_workflows`, `actions_workflow`; b) keep, prose only. Recommended a).
D15. Inbox kind `deploy` and Domain `deploys`: a) keep, gloss; b) rename (`deploy-approval`, `approvals`). Recommended a).
D16. Spec field `context`: a) keep; b) rename `background`. Recommended a).
D17. Jira words: a) keep the levels table, fix "Jira task" to "ticket", caption "Jira analogy"; b) drop Jira words. Recommended a).
D18. Jira key on the Workflow row (`jira`, `pr.jira_required`): a) stays on the Workflow; b) moves to the Plan. Recommended a).
D19. owner: a) GitHub login only; b) leave. Recommended a).
D20. Tier: a) model only, "urgency" for colours; b) leave. Recommended a).
D21. Phase, stage, step: a) one entry; engine items "engine step E7"; b) a) plus relabel the merge algorithm; c) leave. Recommended b).
D22. Session: a) cpd Session, "tmux session", "pi session" in full; b) leave. Recommended a).
D23. Locked: a) "Locked", "Plan lock", "file lock"; b) leave. Recommended a).
D24. waiting counts: a) Job display `held`, status-line count `ready`; b) leave. Recommended a).
D25. Retired entries (Reviewer, Grounded Lead, sync Job): a) delete; b) "Retired" section. Recommended b).
D26. Avoid words that are commands (`ask`, `retry`, `board`, `config`, `install`, `run`): a) "as a noun" on the line; b) drop. Recommended a).
D27. docs/cockpit-parity.md and BACKLOG.md:6 (CI bullet): a) archive the file, delete the bullet; b) keep both. Recommended a).
D28. Guard against retired words (pro-d136): a) test per retired word like skill.test.ts:106-112; b) none. Recommended a).

## Proposed glossary structure

Entry format for every entry: term, definition, `_Avoid_`, where decided. Kinds: entity, phase, stage, step, state, role, mechanism, policy, other, retired. No definitions yet.

1. How to read this glossary: entry format; the levels table (Context, Plan, Workflow, Job, with a "Jira analogy" caption); the rule "PR means only the pull request"; "history keeps the old words".
2. Work units: Context (entity); Plan (entity); Workflow (entity); Job (entity); Job kind (other); Open Job, Update Job, Review Job (entity); Spec, Worker Spec (entity); Dependency and Workflow need (entity); Shape (other); Stacked Workflow and base (entity); Lead Branch, Lead worktree (entity); Attempt (entity); Ledger (entity); Pending work (other).
3. Job and Workflow statuses: Draft, Locked, Ready, Merged, Skipped (state); Workflow status words (state); Escalation (entity).
4. The CPD flow: Phase, stage, step (other); phases grounding, plan, dispatch, deploy, done (phase); stages Context, Plan, Deploy (stage); steps Grounding, Clarifying, Specifying, Dividing, Sequencing, Approving, Dispatching, Mechanical checks, the opening step, Reviewing, Monitoring, Addressing (step); Plan review (entity); CPD (other).
5. GitHub words: pull request, draft pull request, pull request checks, pull request review, squash merge, GitHub Actions workflow, GitHub Actions run (other); PR poll, Feedback item, PR post (entity).
6. Roles: User, Manager, Lead, Worker (role); Brief (entity); Tier (other); Context update (entity).
7. Runtime: Session, tmux session, pi session (entity); Event (entity); Inbox, Inbox item (entity); Cockpit and its columns, Pulse (entity); Engine, engine step (mechanism); message board, board post (entity); Resource, Slot, chat slot, Tenant (other); MCP server, Attach (other).
8. Checks and gates: Gate, Quick check, Mechanical checks (mechanism); CI (other).
9. Policy: Policy, Scope, Domain, Policy key, Exposure, Repo policy file, Machine policy, Resolved policy, Known owner, Fallback, owner (policy).
10. Axioms and marks: Axiom, φ (other).
11. Retired words: cpd 1 Workflow = Plan, PR phase, Reviewer, Grounded Lead, sync Job (retired).

## What the audit could not check

- Whether code, not text, matches any "wrong" claim: no part ran the cockpit, the engine or the tests (Method, V "Limits"); I ran no test either.
- `docs/implementing.md` 283-686 and `docs/interface.md` generated columns (V did not read them as vocabulary); ADR bodies beyond the first paragraph.
- Everything S, R, L, T, X read only by regex at matched lines: comment, string and local-variable uses of `PR` were sampled (S sampled 36 comment lines), so per-line senses are partly guesses; 294 of 501 test files were searched, not opened.
- The tails of the longest Context records (U), and the Spec "TERMS" lists of the parts, which I did not have: the vocabulary covers the terms in the parts' `## Terms` tables, not the Spec's own list.
- Live data effects of a rename on this machine (S counts rows in `~/.cpd/cpd.db`): I did not repeat them; stored fingerprints over the keys `pr` and `prs` (S) are unverified by me.
- Which sense a `mixed` token has in prose beyond the individually listed ones: each part decided them by reading one sentence.
- A record of what the User said is U's reading of the Context file; the User's own sentences are quoted only where U quotes them.
