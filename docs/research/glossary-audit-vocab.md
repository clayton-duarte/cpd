# Glossary audit: CONTEXT.md, README and the vocabulary docs

Attempt `aud-vocab-a1` of Job `aud-vocab`, Plan `glassary`. Read-only audit: nothing but this file was changed. Decisions behind it: Context records pro-d136 and pro-d138 (the abbreviation PR means only the GitHub pull request; the cpd object that was also called PR becomes **Workflow**, Plan > Workflows > Jobs). Sense codes are exactly the six of the Spec: `object`, `pull request`, `phase`, `step`, `mixed`, `other`. "After the rename" assumes the decision and is a proposal; the User decides every contested term.

Reading rule used in every table: a verdict ignores the purely mechanical `object` to Workflow rewrite (that is a RULE in `## Fix list`, counted in `## PR occurrences`), unless the entry has another defect too.

## Coverage

Part: CONTEXT.md, README.md, BACKLOG.md, docs/implementing.md, docs/versioning.md, docs/mark.md, docs/ui-preview.md, docs/cockpit-parity.md, docs/interface.md (Description column only), and the title, status and first paragraph of each docs/adr/*.md. Line counts from `wc -l`.

| File | Lines | Range | How read |
|---|---|---|---|
| CONTEXT.md | 323 | 1-323 | read in full, every line (78 entries, 60 `_Avoid_` lines) |
| README.md | 54 | 1-54 | read in full |
| BACKLOG.md | 16 | 1-16 | read in full |
| docs/implementing.md | 686 | 1-282 | read in full |
| docs/implementing.md | 686 | 283-686 | skimmed: read only the headings (`rg -n '^#'`), every line that holds a PR, pull request, `pr` or workflow token, and lines 407-417 in full. Not read word by word: the fake tmux, git route, Fx fixtures, test kit, leak check, dependencies, install, adapter swap, portability check, scoped loop, debuginfo, build jobs, version string, compact-tools, ask_user_question and pi version sections. Vocabulary there, other than PR and workflow, is unaudited |
| docs/versioning.md | 45 | 1-45 | read in full |
| docs/mark.md | 13 | 1-13 | read in full |
| docs/ui-preview.md | 33 | 1-33 | read in full |
| docs/cockpit-parity.md | 232 | 1-232 | read in full |
| docs/interface.md | 172 | 1-172 | Description column only (column 2 of the table, lines 6-172, by `awk -F'\|' '{print $3}'`); the Command, Callers and Cockpit columns are generated and were not read as vocabulary. Only the Description cells with a PR token were read word by word; the others were skimmed by the same `rg` |
| docs/adr/0001..0014 | 14 files | title, status, first paragraph (lines 1-12 of each) | read; history, no change proposed. Where a term was born is in `## Terms` (rows that cite docs/adr) |

Lines read in full: 323 + 54 + 16 + 282 + 45 + 13 + 33 + 232 = 998. Skimmed: 404 lines of docs/implementing.md (283-686) and the interface.md Description cells. Not read: docs/contract.md and docs/commands.md (another part's); I looked up single lines of docs/contract.md (:3560, :3963, :1133, :294) and one of docs/adr only to test a claim of this part, and label each use.

Commands behind each count (from the worktree; the numbers are in `## Counts`):

| Count | Command |
|---|---|
| Entries in CONTEXT.md | `rg -n '^\*\*' CONTEXT.md \| wc -l` gives 78 (every entry heading starts a line with `**`) |
| `_Avoid_` lines | `rg -n '^_Avoid_' CONTEXT.md \| wc -l` gives 60 |
| Capital `PR` or `PRs`, per file | `rg -o -w 'PRs?' <file> \| wc -l`: CONTEXT.md 66, README.md 0, BACKLOG.md 1, docs/implementing.md 36, docs/versioning.md 3, docs/mark.md 0, docs/ui-preview.md 3, docs/cockpit-parity.md 22, docs/interface.md 12 (whole file, generated columns included) |
| `pull request` in any case | `rg -oi 'pull requests?' <file> \| wc -l`: CONTEXT.md 15, README.md 2, docs/cockpit-parity.md 1, docs/interface.md 7 (whole file) |
| The token set the occurrence table is complete over | `rg -o '\b(PRs?\|prs\|pr\|pr_\w+\|pull_requests)\b\|[Pp]ull [Rr]equests?' <file>`; every token of this set in the part (interface.md: Description column) is in one row, checked by a script: per-line counts equal, every quote found on its line |
| Workflow in any case | `rg -oiw 'workflows?' <file> \| wc -l`: CONTEXT.md 6, BACKLOG.md 1, docs/cockpit-parity.md 11 |
| Avoid-word live uses | `rg -n -w -i -- '<word>' <file>` per file, with CONTEXT.md's `_Avoid_` lines removed (`rg -v '^[0-9]+:_Avoid_'`); counts in `## Avoid lists` are lines, not tokens |
| `.github` absent | `ls .github` in this worktree: `No such file or directory` |
| PROTOCOL and migrations | `rg -n 'PROTOCOL: ' core/src/events/kind.rs` gives `pub const PROTOCOL: u32 = 17;`; `ls core/src/store/migrations/*.sql \| wc -l` gives 75 |

Limits, all unverified unless stated: where I say stale I compared with the one line of docs/contract.md or the git log named in the row, not with the whole contract. The commit message of 32bb8cd3 (`git show --stat 32bb8cd3`) names the red test "the widgets column is the machine header then the Inbox, no PRs section"; I did not run the cockpit.

## Terms

One row per term or word-to-watch used in more than one sense, used loosely, stale, wrong or colliding. Double quotes in the third column are found on the cited line; single quotes elsewhere are not claims about that line.

| Term | Where | Sense as used here | Collision | Verdict | Proposed wording |
|---|---|---|---|---|---|
| PR (object) | CONTEXT.md:112 | the cpd object: "A PR may need other PRs of the same Plan" | upstream GitHub: PR is a pull request; internal: PR phase (29), PR step (71), PR poll (185), PR post (193) | wrong (after pro-d136) | Workflow for the object; PR only for the pull request. Why: one word, one thing |
| Workflow | CONTEXT.md:108 | cpd 1's name for Plan: "the term Workflow is retired" | internal: returns with the new meaning (Plan > Workflows > Jobs); upstream: GitHub Actions workflow | stale | new entry Workflow; delete 'replaces Workflow' (107) and the retirement sentence; write a GitHub Actions workflow in full |
| workflow (Avoid word) | CONTEXT.md:109 | "_Avoid_: workflow, task, project, run" under Plan | internal: now a live term | wrong (after rename) | drop `workflow` from Plan's Avoid list; Workflow gets its own entry |
| workflow (Avoid word) | CONTEXT.md:183 | "daemon, scheduler, watcher per workflow" under Engine | internal: now live | stale | 'watcher per Plan' if the cpd 1 per-Plan watcher is meant |
| every term means what it meant | CONTEXT.md:3 | "every term below that is not marked new or changed means what it meant there" | internal: false for Workflow (cpd 1: Plan) | stale | add: except Workflow, which returns with a new meaning |
| Plan | CONTEXT.md:22 | phase: "The phase where the Lead turns Requirements into a draft graph of Jobs" | Plan stage (50), Plan entity (107); Jira task (108) | overloaded | keep the entity as Plan; write 'plan phase' and 'Plan stage' in every use |
| Plan | CONTEXT.md:51 | stage: "Dividing, Sequencing, Approving, Dispatching" | as above | overloaded | as above |
| Context | CONTEXT.md:39 | stage: "Grounding, Clarifying, Specifying" | Context entity (87), Context record (91), cpd 1 phase name (18); Jira Epic (88) | overloaded | stage and entity kept; the cpd 1 sense is history |
| context (lowercase) | CONTEXT.md:132 | Spec content: "goal, context, files it touches" | Context entity; 'no other context' (116) means an agent's context window | loose | 'background' for the Spec section; 'context window' for 116 |
| Grounding | CONTEXT.md:18 | phase: "The first phase of every Plan" | Grounding step (41); Grounded Lead (103) | overloaded | make the step entry point to the phase entry |
| Deploy | CONTEXT.md:66 | stage: "Mechanical checks, PR, Reviewing and Monitoring, Addressing" | phase `deploy` (15); Domain `deploy` (268); upstream: deploy is shipping to an environment | overloaded | say in the Domain entry that `deploy` is a policy slot |
| Dispatch | CONTEXT.md:26 | phase: "The phase that starts when the Plan is locked" | Dispatching step (62); 'Dispatch column' (186); `job dispatch` (docs/implementing.md:45); upstream: GitHub `workflow_dispatch` | overloaded | heading 'Dispatch (phase)' like its siblings; keep 'the Dispatch column' only if the column still exists |
| PR (phase) | CONTEXT.md:30 | the post-open phase: "until all are merged" | now the phase `deploy` (15) | stale | delete the entry; the contract's phase is `deploy` |
| PR (step) | CONTEXT.md:72 | the step: "The pull request is opened" | none after the rename | ok (after rename) | keep; rewrite the object words |
| Reviewing, review | CONTEXT.md:75 | step: "review the open pull request" | Review Job (135), Reviewer (227), Plan review (60), pull request review on GitHub | overloaded | three reviews, three names: Plan review, Review Job, pull request review |
| Monitoring | CONTEXT.md:74 | named in "(step, with Monitoring)" and in the Deploy list (66) | none | loose | give it an entry or drop it from the headings |
| Merged | CONTEXT.md:248 | Job state: "The terminal success state of a Job" | pull request merged (30, 182); git merge (124); 'per merge' in the Gate (236); `pr merge` squash merge | overloaded | write 'Merged' only for the Job and 'the pull request merges' for GitHub |
| merge (verb) | CONTEXT.md:124 | git: "merges the PR's base into the Lead Branch" | the Job's own merge, a fast-forward (79); squash merge (194) | loose | three verbs: merge the base, fast-forward the Lead Branch, squash-merge the pull request |
| open | CONTEXT.md:124 | Job kind: "a Job of kind `open`" | status `open` (30); open Inbox items (174); GitHub open pull request (186); 'never opens by itself' (152) | overloaded | keep Open Job; say 'status open' and 'open Inbox item' in full |
| Draft | CONTEXT.md:152 | Job: "A Job the User has not approved yet" | upstream GitHub draft pull request (124 'never a draft') | overloaded | write 'draft pull request' in full where GitHub's is meant |
| Locked | CONTEXT.md:156 | Job state: "The state of every Job that is not a draft" | `plan lock` (60), kernel `flock` (288), Resource's Avoid word 'lock' (289); upstream GitHub issue locking | loose | none beyond dropping 'lock' from Resource's Avoid |
| Session | CONTEXT.md:162 | "A pi session cpd knows about" | tmux session (docs/adr/0009-cockpit-replaces-herdr.md:7); Context 'Not a session' (88) | overloaded | say 'pi session' first, 'tmux session' in full |
| waiting, blocked | CONTEXT.md:162 | agent states "waiting (an agent waits for another agent" and "blocked (the User has to act" | Workflow status `waiting` (docs/interface.md:70 'Start a waiting PR'); Avoid 'blocker' (141, 231) | overloaded | the Workflow status `waiting` needs a qualifier; keep the Session states |
| Event | CONTEXT.md:166 | "An append-only record of something that happened" | GitHub events; tmux hooks; Avoid 'message' (167) yet 'delivers some to its Lead as messages' | loose | drop 'message' from Event's Avoid, or call the delivery an 'Event message' |
| message | CONTEXT.md:216 | "The message the pi adapter adds at the start of a Manager's turn" | Event Avoid (167); Slack message (194); 'first message' (100) | loose | Context update is the one message that needs the word; see Event |
| Gate | CONTEXT.md:236 | "only when the PR's Policy sets `pr.gate" | upstream: required checks; internal: Quick check; contract key is `gate.pr` (docs/contract.md:3963) | stale | name the contract's keys (`gate.merge`, `gate.pr`); drop `pr.gate` |
| check, checks | CONTEXT.md:237 | Avoid "CI, checks" under Gate | Mechanical checks (68), CI checks of the pull request (75, 194), Quick check (239) | overloaded | Avoid only 'CI' as a synonym; 'checks' for GitHub's, 'Mechanical checks' for cpd's |
| CI | CONTEXT.md:236 | "the repo's CI is the check (hs-d292)" | upstream GitHub Actions; this repo has no `.github` (verified); the repo policy says it runs no GitHub Actions | stale | 'the repo's CI, where it has one'; unverified for other repos |
| Quick check | CONTEXT.md:240 | "The repository's `pre-commit` hook" | docs/contract.md:3560 calls `pre-commit` 'the Gate' for `gate.merge` | wrong | align one side: Quick check is a Gate of `gate.merge`, or the contract drops the name |
| Spec | CONTEXT.md:133 | Avoid "brief, prompt, ticket" | Brief entry (207); 'brief' used live (100) | loose | drop `brief` from Spec's Avoid and say 'Not a Brief' as the Brief entry does (208) |
| Epic, Ticket, Task | CONTEXT.md:9 | levels table maps the Jira word "Epic" to Context | Avoid epic (89), ticket (209), task (117, 109, 175); upstream Jira Epic, Ticket, Task | overloaded | caption the table 'Jira terms, mapped' |
| owner | CONTEXT.md:201 | Avoid "operator, owner" under User | GitHub owner (264, 318); 'The human who owns the ticket' (200) | overloaded | Avoid 'owner' only in the User's sense |
| Scope | CONTEXT.md:272 | "Where a Domain's value is set" | 'out of scope' (132); Policy key Avoid 'scope' (285) | loose | none; keep the Avoid for Policy key |
| Policy | CONTEXT.md:264 | "How cpd and its agents work for one GitHub owner or repo" | `cpd config`, `config.toml` (docs/implementing.md:32) while Avoid lists 'config' (265) | overloaded | Avoid 'config' only for the Policy; say `cpd config` is machine config |
| Domain | CONTEXT.md:268 | "One typed slot of a Policy, owned by cpd's code" | Deploy stage (66); Domain `pr` (see PR occurrences) | loose | list Domains in the contract only |
| Slot | CONTEXT.md:292 | "The number of available concurrent units of a determined resource: one concurrent unit of it." | Resource (288) 'limited number of slots'; chat slot of the cockpit (docs/ui-preview.md:17) | wrong | 'One concurrent unit of a Resource' |
| Skipped | CONTEXT.md:299 | "A Ready Job that E3 could not start for want of a pool" | upstream GitHub check conclusion `skipped`; Ready has no entry | loose | define Ready; say 'Skipped Job' |
| Attach | CONTEXT.md:310 | "one explicit `cpd mcp attach <name>`" | tmux attach (README.md:22 'Install and attach', `cpd tui --attach`) | overloaded | 'attach an MCP server' versus 'attach the cockpit' |
| Distilled Context | CONTEXT.md:100 | "The curator at work: the distiller runs once per batch of uncurated records" | digest (216); Avoid 'model digest' (101) | overloaded | the term names a process, not a Context: 'Distiller' |
| digest | CONTEXT.md:216 | "It ends with the **digest**: a `Plans:` section" | digest of the Context (18, 100) | overloaded | 'Plans digest' for the Manager's; 'Context digest' for the Context's |
| Pull Request (capital) | CONTEXT.md:112 | "the Pull Request opened from that branch" | lowercase 'pull request' (3, 72) | loose | lowercase everywhere (also 182, 227, 244) |
| PR level | CONTEXT.md:236 | "run by cpd only at the PR level" | the levels table (7-11) has no PR level | loose | 'at the Workflow level' and add the level to the table |
| levels table | CONTEXT.md:5 | "The levels, the cpd objects and their roles" lists three levels | the new level Workflow sits between Plan and Job | stale | add a Workflow row; README.md:11 restates the same table |
| Lead worktree | CONTEXT.md:112 | "a Lead Branch and Lead worktree" | gate worktree (236), Worker's worktree (224) | loose | give it a line in the Lead Branch entry |
| base | CONTEXT.md:194 | "(base PR not yet merged)" | base branch (112), 'the PR's base' (124) | loose | a Stacked entry: a Workflow whose base is another Workflow's Lead Branch |
| ready | CONTEXT.md:124 | "opens the pull request, ready, never a draft" | Ready Job (124), 'when the PR is ready' (194) | overloaded | 'Ready Job' versus 'ready for review' |
| record | CONTEXT.md:92 | Context record kinds: "a decision (which may supersede another)" | Ledger (148) 'durable, append-only record'; Event (166) | loose | none; three stores say record |
| state, status | CONTEXT.md:156 | "The state of every Job that is not a draft" | Workflow statuses in the Spec; Session 'agent state' (162); 'terminal success state' (248) | loose | one word: status for rows, state for the Session |
| phase, stage, step | CONTEXT.md:15 | "A Plan moves through the phases" | no entry defines the three words | loose | one entry 'Phase, stage, step' |
| step | CONTEXT.md:117 | Avoid "card, task, step" under Job | Deploy steps (66); engine steps (docs/implementing.md:18); 'build step' (docs/implementing.md:95) | overloaded | Avoid 'step' only for 'a Job is a step' |
| thread | CONTEXT.md:190 | "a review thread, an issue comment or a review with a body" | Avoid 'subagent, executor, thread' under Worker (225); upstream GitHub review thread | loose | none; the Avoid is for Worker only |
| retry | CONTEXT.md:145 | Avoid "retry, run" under Attempt | `job retry` (docs/implementing.md:39) | loose | Avoid 'retry' only as a noun for Attempt |
| ask | CONTEXT.md:231 | Avoid "blocker, ask" under Escalation | `cpd ask` (docs/implementing.md:7), module `ask` (docs/implementing.md:42) | wrong | the command is `ask`; the Avoid word means the noun 'an ask' |
| board | CONTEXT.md:149 | Avoid "database, board, tracker" under Ledger | test module `board` (docs/implementing.md:129); upstream Jira board | loose | none; unverified what `board` holds |
| post | CONTEXT.md:194 | "One message announcing a PR for review in Slack" | `cpd pr reply` posts a comment (190) | loose | 'Slack post' versus 'reply' |
| Dispatch column | CONTEXT.md:186 | "feeds the Dispatch column" | git log 32bb8cd3: 'no PRs section' in the widgets column | stale | drop it (commit message verified; code not run) |
| Distinct from the PR phase | CONTEXT.md:112 | "Distinct from the PR phase" | the phase no longer exists | stale | delete |
| Grounded Lead | CONTEXT.md:104 | "No longer created (B3)" | none | stale | delete, or move to history |
| Reviewer | CONTEXT.md:227 | "Removed by R19" | born in docs/adr/0002-review-before-merge.md:1 | stale | delete: a removed term in the live glossary adds a word to avoid |
| Known owner | CONTEXT.md:318 | "The known owners on this instance are `clayton-duarte`" | instance data | loose | point to `cpd policy show`; no names in the glossary |
| CI job | docs/implementing.md:413 | "CI's `rust` job runs the same command" | cpd Job; `.github` absent | stale | delete the sentence or say 'where CI exists' |
| CI | BACKLOG.md:6 | "Move CI from `ubuntu-24.04` to `ubuntu-26.04`" | repo has no `.github`; policy: no GitHub Actions | stale | ask the User whether the bullet is still wanted |
| Remote workflows | BACKLOG.md:16 | "Remote workflows: other machines' trees in the cockpit over SSH" | cpd 1 meaning of Workflow (Plan); the new meaning returns | stale | 'Remote trees' |
| Workflow (cpd 1) | docs/cockpit-parity.md:59 | "the Workflows of the chat sessions in the cockpit" | the file audits cpd 1 against cpd 2; Workflow is the reference's word | stale | leave as history or archive the file; see Fix list |
| PROTOCOL, migrations | docs/versioning.md:5-6 | "Today it is **13**" and, on line 6, "Today it is **45**" | code: PROTOCOL 17 and 75 migration files (commands under Coverage) | wrong | state the rule, not today's numbers |
| mark, logo | docs/mark.md:13 | "The logo itself is defined soon" | CONTEXT.md:83 Avoid 'cpd logo (the logo is defined soon)' | ok | consistent |
| Workflow born | docs/adr/0001-sqlite-workflow-state.md:5 | "SQLite is the single source of truth for a Workflow" | cpd 1 Workflow = Plan | ok | history: first use in ADR-0001; docs/adr/0011-contexts-and-plans.md:7 says Plan is 'today's Workflow' |
| PR object born | docs/adr/0011-contexts-and-plans.md:7 | "one repo and one PR for now" | the object: a Plan was one PR before R34 | ok | history: origin of the object |
| Reviewer born | docs/adr/0002-review-before-merge.md:1 | "Attempts are reviewed before they are merged, by a forked read-only Reviewer" | removed by R19 | ok | history |
| Ledger born | docs/adr/0004-beads-ledger.md:8 | "Each Workflow's Jobs, Dependencies, Attempts, reviews" | cpd 2's Ledger is SQLite | ok | history |

Terms fine, in one line: Clarifying, Specifying, Approving, Dividing, Sequencing, Dispatching, Context record, Context store, Job kind, Update Job, Dependency, Attempt, Ledger, Inbox, Inbox item, Cockpit, Engine, Feedback item, PR poll and PR post as words, User, Manager, Pending work, Lead, Worker, Escalation, Tier, Axiom, Exposure, Policy key, Resource, Tenant, Machine policy, MCP server, Resolved policy, Fallback, φ, CPD, README.md, docs/mark.md, docs/ui-preview.md (apart from its PR words).

## Entries

One row per entry of CONTEXT.md, in file order. Kind: phase, stage, step, entity, role, state, mechanism, policy, other (Policy entries are `other`). Verdict: ok, stale, contradicts, overloaded, restates. "PR=object" in a note is the mechanical rename, not a defect of the entry. The row count equals the 78 entry headings of `rg -n '^\*\*' CONTEXT.md`.

| Term | Kind | One-line summary | Verdict | Note |
|---|---|---|---|---|
| Grounding (phase) | phase | first phase: the Lead reads the records and interviews the User | overloaded | CONTEXT.md:18: same name as the Grounding step (41) and as the retired Grounded Lead (103); says "Replaces cpd 1's" phase name |
| Plan (phase) | phase | the Lead turns Requirements into a draft graph of Jobs, the User approves | overloaded | CONTEXT.md:22: same name as the Plan stage (50) and the Plan entity (107) |
| Dispatch | phase | starts when the Plan is locked; ends when every PR is open | overloaded | CONTEXT.md:26: heading lacks '(phase)' unlike its siblings; same stem as Dispatching (62); PR=object x3 (rename rule) |
| PR (phase) | phase | from every PR open until all merged; then the Plan is done | stale | CONTEXT.md:15: the phases are "grounding → plan → dispatch → deploy → done"; this phase is now `deploy` |
| CPD | other | Continuous Parallel Delivery; flow of three stages | ok | CONTEXT.md:36: "the cockpit's three columns" are named Context, Plan, Deploy here; docs/cockpit-parity.md:65 gives a column the title Dispatch (unverified in the running cockpit) |
| Context (stage) | stage | Grounding, Clarifying, Specifying | overloaded | CONTEXT.md:39: same name as the Context entity (87) |
| Grounding (step) | step | reading the Context's records and the User's material | restates Grounding (phase) CONTEXT.md:18 | CONTEXT.md:42: one line; the phase entry holds the same content |
| Clarifying | step | the interview, until unknowns are minimised | ok | CONTEXT.md:45 |
| Specifying | step | writing Requirements, assumptions and records | ok | CONTEXT.md:48: "Requirements" has no entry |
| Plan (stage) | stage | Dividing, Sequencing, Approving, Dispatching; phases plan and dispatch | overloaded | CONTEXT.md:51: shares phase `dispatch` with the Deploy stage (66) |
| Dividing | step | splitting the work into PRs and Jobs | stale | CONTEXT.md:54: PR=object x2 and `pr add`: becomes Workflows and a workflow command |
| Sequencing | step | ordering with --needs and PR needs | stale | CONTEXT.md:57: PR=object; "needs" has no entry |
| Approving | step | the Plan review, then plan lock | ok | CONTEXT.md:60: "Plan review" has no entry |
| Dispatching | step | the engine runs the Jobs | overloaded | CONTEXT.md:63: same stem as the Dispatch phase (26) |
| Deploy (stage) | stage | Mechanical checks, PR, Reviewing and Monitoring, Addressing | overloaded | CONTEXT.md:66: also phase `deploy` (15) and Domain `deploy` (268); Monitoring has no entry; lists a step named PR |
| Mechanical checks | step | checks cpd runs at an Attempt's submit, by Job kind | contradicts docs/contract.md:3963 | CONTEXT.md:69: "the PR's `pr.gate` is `local`"; the contract's keys are `gate.merge` and `gate.pr`; restates the Gate (236) |
| PR (step) | step | the pull request is opened by the open Job's merge | ok | CONTEXT.md:72: after the rename the step name PR means the pull request; PR=object x1 (rename rule) |
| Reviewing (step, with Monitoring) | step | people, the review bot and CI review the open pull request; the engine polls it | overloaded | CONTEXT.md:75: three reviews (Plan review, Review Job, pull request review); "CI review the open pull request" says CI reviews |
| Addressing | step | a feedback fix is one work Job appended to the open PR | ok | CONTEXT.md:79: mixed x5 (see PR occurrences); mechanism text, much of it also in the contract (unverified) |
| φ | other | cpd's mark | ok | CONTEXT.md:82: points to docs/mark.md |
| Context (entity) | entity | the parent shared by two or more Plans | overloaded | CONTEXT.md:88: "Not a session."; Avoid epic (89) while the levels table says Epic (9) |
| Context record | entity | one typed append-only entry of a Context's knowledge | ok | CONTEXT.md:92 |
| Context store | mechanism | a Context's records in the machine's database | ok | CONTEXT.md:96: history inside the entry ("imported and archived by migration 0007") |
| Distilled Context | mechanism | the distiller curates, compresses and retires records | restates docs/contract.md §4.6 | CONTEXT.md:100: about 230 words; names a process, not a Context; docs/implementing.md:56 cites contract §4.6 for the same (unverified, contract not read) |
| Grounded Lead | role | retired: the Manager replaced it | stale | CONTEXT.md:104: "No longer created (B3)" |
| Plan (entity) | entity | one deliverable over one or more PRs, in exactly one Context | stale | CONTEXT.md:108: "the term Workflow is retired"; Avoid workflow (109); Workflow returns |
| PR (entity) | entity | the part of a Plan in one repository, tied to its Lead Branch | stale | CONTEXT.md:112: becomes Workflow; "Distinct from the PR phase" goes with the phase |
| Job | entity | a self-contained unit of work defined by a Spec | overloaded | CONTEXT.md:116: Avoid task and step; upstream GitHub Actions job (docs/implementing.md:413) |
| Job kind | other | work, docs, test, impl, verify, open, update | restates docs/contract.md §2.2 | CONTEXT.md:120: docs/contract.md:1133 cites §2.2 Job kinds; shape and `free` have no entry |
| Open Job | entity | the last Job of every started PR, added by cpd | ok | CONTEXT.md:124: PR=object x3, `pr open` mixed |
| Update Job | entity | a Job of kind update for an open pull request that conflicts | ok | CONTEXT.md:128 |
| Spec | entity | the complete description of a Job | overloaded | CONTEXT.md:133: Avoid brief while Brief is an entry (207); CONTEXT.md:132: "goal, context, files it touches" uses context |
| Review Job | entity | a Job whose Worker is a fork of the Lead; findings go to a file | overloaded | CONTEXT.md:136: one of three review senses (see Terms) |
| Dependency | entity | a Job may start only after another Job of the same PR is Merged | ok | CONTEXT.md:140: "needs" for order across PRs has no entry |
| Attempt | entity | one execution of a Job by one fresh Worker session | ok | CONTEXT.md:144: Avoid retry (145) but `job retry` exists (docs/implementing.md:39) |
| Ledger | entity | durable append-only record of every Plan's Jobs and Events | ok | CONTEXT.md:148: Avoid board (149); `board` is a test module (docs/implementing.md:129) |
| Draft | state | a Job the User has not approved yet | overloaded | CONTEXT.md:152: upstream GitHub draft pull request (124) |
| Locked | state | the state of every Job that is not a draft | overloaded | CONTEXT.md:156: PR=object; also `plan lock` (60) |
| Session | entity | a pi session cpd knows about, with a role and an agent state | overloaded | CONTEXT.md:162: also tmux session; CONTEXT.md:88: "Not a session" |
| Event | entity | an append-only record of something that happened | overloaded | CONTEXT.md:166: Avoid message (167) while it "delivers some to its Lead as messages" |
| Inbox | entity | the single source of needs-you | ok | CONTEXT.md:170 |
| Inbox item | entity | one thing that needs the User | ok | CONTEXT.md:174: "a pull request of the User" |
| Cockpit | entity | the User's interface: one tmux window | ok | CONTEXT.md:178 |
| Engine | mechanism | the one cpd watch per machine | stale | CONTEXT.md:182: PR=object x2; CONTEXT.md:183: "watcher per workflow" |
| PR poll | mechanism | the engine's 5 s poll of the User's open pull requests | stale | CONTEXT.md:186: "feeds the Dispatch column"; commit 32bb8cd3 removed the PRs section (commit message, not run) |
| Feedback item | entity | one open thing a person said on a pull request | restates docs/contract.md §2.2 | CONTEXT.md:190: says "Defined once in `docs/contract.md` §2.2": cut to a pointer |
| PR post | entity | one Slack message announcing a pull request | restates docs/contract.md §2.2 | CONTEXT.md:194: says "Defined once in `docs/contract.md` §2.2": cut to a pointer |
| User | role | the human who owns the ticket | overloaded | CONTEXT.md:201: Avoid owner while GitHub owner is a Policy term (264) |
| Manager | role | the higher-level Lead of a Context | ok | CONTEXT.md:204: mixed x1 ("PR opened or merged") |
| Brief | entity | a Context record of kind brief | overloaded | CONTEXT.md:208: Avoid brief appears under Spec (133) |
| Pending work | other | in-flight work of a Context's active Plans | ok | CONTEXT.md:212: PR=object x2 |
| Context update | entity | the message the pi adapter adds at a Manager's turn | overloaded | CONTEXT.md:216: Avoid notification and poll; "message" is Event's Avoid word; digest sense differs from line 100 |
| Lead | role | the agent that turns User intent into Specs | ok | CONTEXT.md:220: "orchestrates the Plan" uses the Avoid word's stem |
| Worker | role | an agent session that executes one Attempt of one Job | ok | CONTEXT.md:224 |
| Reviewer (removed) | role | removed by R19 | stale | CONTEXT.md:227: "Removed by R19"; history is docs/adr/0002-review-before-merge.md:1 |
| Escalation | entity | a question an agent raises to its parent | overloaded | CONTEXT.md:231: Avoid ask while `cpd ask` exists (docs/implementing.md:7) |
| Gate | mechanism | the repository's mechanical checks, run at the PR level | stale | CONTEXT.md:236: `pr.gate` is not the contract's key (docs/contract.md:3963); Avoid CI and checks (237) while the entry says "the repo's CI is the check" |
| Quick check | mechanism | the repository's pre-commit hook, run at a Worker's submit | contradicts docs/contract.md:3560 | CONTEXT.md:240: the hook; docs/contract.md:3560 names `pre-commit` "the Gate" for `gate.merge` |
| Lead Branch | entity | the branch of one PR that accumulates its Merged Jobs | stale | CONTEXT.md:244: PR=object x2, pull request x1 |
| Merged | state | the terminal success state of a Job | overloaded | CONTEXT.md:248: PR=object; pull request merge, git merge and squash merge exist beside it |
| Tier | other | a two-character code naming model and effort | ok | CONTEXT.md:252: Avoid level (253); the levels table (7) is a different sense |
| Axiom | entity | one of the creator's core rules, from AGENTS.md | ok | CONTEXT.md:258: Avoid rule (259) while the text says "core rules" |
| Policy | entity | how cpd and its agents work for one GitHub owner or repo | overloaded | CONTEXT.md:264: Avoid config and settings (265); `cpd config` exists (docs/implementing.md:32) |
| Domain | other | one typed slot of a Policy | overloaded | CONTEXT.md:268: Domain `deploy` and `pr` collide with the Deploy stage and the object |
| Scope | other | where a Domain's value is set | overloaded | CONTEXT.md:272: Avoid scope under Policy key (285); CONTEXT.md:132: "out of scope" in Spec |
| Repo policy file | other | the repo scope of a Policy | restates Scope CONTEXT.md:272 | CONTEXT.md:276: "The repo scope of a Policy"; two blank lines follow (277-278) |
| Exposure | other | the Scopes from which a Domain may be set | ok | CONTEXT.md:280: key `pr.post` mentioned as pull request |
| Policy key | other | what a Policy is resolved for: an owner, or an owner and a repo | stale | CONTEXT.md:284: PR=object x2 |
| Resource | other | anything with a limited number of concurrent slots | ok | CONTEXT.md:288: Avoid lock (289) while it holds a kernel `flock` |
| Slot | other | one concurrent unit of a resource | contradicts CONTEXT.md:292 | CONTEXT.md:292: "The number of available concurrent units of a determined resource: one concurrent unit of it." |
| Tenant | other | the Job or user occupying a slot | ok | CONTEXT.md:296: "The Job or user": lowercase user is not the User |
| Skipped | state | a Ready Job that E3 could not start for want of a pool | overloaded | CONTEXT.md:299: Ready has no entry; GitHub check conclusion skipped (upstream) |
| Machine policy | other | the machine Scope's file host.toml | ok | CONTEXT.md:302 |
| MCP server | other | a remote server cpd may attach to a Session | ok | CONTEXT.md:306 |
| Attach | other | giving an MCP server to a Session | overloaded | CONTEXT.md:310: tmux attach (README.md:22 "Install and attach") |
| Resolved policy | other | the Policy of one Policy key, with sources | ok | CONTEXT.md:314 |
| Known owner | other | an owner with a policy owner.toml file | restates $CPD_HOME/policy | CONTEXT.md:318: "The known owners on this instance are `clayton-duarte`": instance data |
| Fallback | other | the owner whose policy is used when an unknown owner is confirmed | restates $CPD_HOME/policy | CONTEXT.md:322: "`clayton-duarte` on this instance": instance data |

## Same word, several senses

Every word that names two or more entries, or has two or more senses inside CONTEXT.md alone.

| Word | Senses (entry or line) | Verdict |
|---|---|---|
| PR | the pull request (PR poll CONTEXT.md:185, PR post CONTEXT.md:193); the object (PR entity CONTEXT.md:111); the old phase (CONTEXT.md:29); the Deploy step (CONTEXT.md:71) | four senses in one file; after the rename PR keeps the pull request and the step |
| Plan | phase (CONTEXT.md:21), stage (CONTEXT.md:50), entity (CONTEXT.md:107) | three entries; say phase, stage or entity in every use |
| Context | stage (CONTEXT.md:38), entity (CONTEXT.md:87), cpd 1 phase name (CONTEXT.md:18), Context record, Context store, Distilled Context, Context update | one stem, seven uses; the cpd 1 sense is history |
| Grounding | phase (CONTEXT.md:17), step (CONTEXT.md:41), Grounded Lead (CONTEXT.md:103) | the step restates the phase |
| Deploy | stage (CONTEXT.md:65), phase `deploy` (CONTEXT.md:15), Domain `deploy` (CONTEXT.md:268) | the Domain is a policy slot, unrelated |
| Dispatch | phase (CONTEXT.md:25), step Dispatching (CONTEXT.md:62), 'Dispatch column' (CONTEXT.md:186), command `job dispatch` (docs/implementing.md:45) | the column sense is stale |
| Review | step Reviewing (CONTEXT.md:74), Review Job (CONTEXT.md:135), Reviewer removed (CONTEXT.md:227), Plan review (CONTEXT.md:60), the review of the pull request on GitHub (CONTEXT.md:75) | four live senses |
| Merged, merge | Job state (CONTEXT.md:248), pull request merged (CONTEXT.md:30), merging the base (CONTEXT.md:124), the Job's own merge to the Lead Branch (CONTEXT.md:79), squash merge (CONTEXT.md:194) | five |
| open | Job kind (CONTEXT.md:123), status of the object (CONTEXT.md:30), open pull request (CONTEXT.md:186), open Inbox item (CONTEXT.md:174), 'never opens by itself' (CONTEXT.md:152) | five |
| Draft | Job not approved (CONTEXT.md:151), GitHub draft pull request (CONTEXT.md:124) | two |
| Gate | the Gate (CONTEXT.md:235), Quick check as the Gate of `gate.merge` (docs/contract.md:3560), 'no PR Gate' (CONTEXT.md:79) | contradiction with the contract |
| Session | pi session (CONTEXT.md:161), tmux session (docs/adr/0009-cockpit-replaces-herdr.md:7), 'Not a session' (CONTEXT.md:88) | two |
| Event | cpd Event (CONTEXT.md:165), GitHub events and tmux hooks (upstream) | upstream collision |
| digest | Context digest (CONTEXT.md:18, 100), Plans digest (CONTEXT.md:216) | two |
| lock | Locked (CONTEXT.md:155), `plan lock` (CONTEXT.md:60), kernel `flock` (CONTEXT.md:288) | three; Resource's Avoid word is a real term |
| check | Mechanical checks (CONTEXT.md:68), Quick check (CONTEXT.md:239), CI checks of a pull request (CONTEXT.md:194), Avoid word under Gate (CONTEXT.md:237) | four |
| Slot | Resource slot (CONTEXT.md:291), Worker slots (CONTEXT.md:288), chat slot (docs/ui-preview.md:17) | three |
| Scope | policy Scope (CONTEXT.md:271), 'out of scope' (CONTEXT.md:132) | two |
| owner | GitHub owner (CONTEXT.md:264), the User 'owns the ticket' (CONTEXT.md:200) | two |
| base | base branch (CONTEXT.md:112), 'the PR's base' (CONTEXT.md:124), 'base PR' (CONTEXT.md:194) | three |
| Ready | Ready Job (CONTEXT.md:124), pull request ready (CONTEXT.md:194) | two |
| post | Slack post (CONTEXT.md:194), reply on the pull request (CONTEXT.md:190) | two |
| state, status | Job state (CONTEXT.md:156), agent state (CONTEXT.md:162), Job status (CONTEXT.md:212) | loose |
| phase, stage, step | defined by use only (CONTEXT.md:15, 36, 37) | no entry |
| poll | PR poll (CONTEXT.md:185), Avoid word under Context update (CONTEXT.md:217) | two |
| record | Context record (CONTEXT.md:91), Ledger (CONTEXT.md:147), Event (CONTEXT.md:165) | three |
| Attach | MCP attach (CONTEXT.md:310), tmux attach (README.md:22) | two |
| Workflow | cpd 1 name for Plan (CONTEXT.md:108), the new object (decision pro-d136) | two, one retired |

## Used but not defined

Terms of art that entries use and no entry defines.

| Term | Used at | Note |
|---|---|---|
| Workflow | CONTEXT.md:108 | defined only as retired; the new meaning has no entry |
| Phase, Stage, Step | CONTEXT.md:15, 36, 37 | kinds named in parentheses only |
| Requirements | CONTEXT.md:18 | capitalised like a term; no entry |
| Plan review | CONTEXT.md:60, 152, 156, 174 | the User's approval of a batch; central, undefined |
| needs | CONTEXT.md:57, 120, 140 | `--needs` and 'PR needs', the order across PRs |
| Shape | CONTEXT.md:120 | `tdd`, `docs`, `config`, `free` (docs/contract.md:294) |
| Stacked, base PR | CONTEXT.md:194 | meaning of 'stacked (base PR not yet merged)' |
| Lead worktree, gate worktree | CONTEXT.md:112, 236 | no entry; Worker's worktree (CONTEXT.md:224) |
| Ready, queued, running, failed | CONTEXT.md:124, 156, 212, 299 | Job statuses; 'Ready Job' is capitalised without an entry |
| Monitoring | CONTEXT.md:66, 74 | in two headings, no entry |
| Distiller | CONTEXT.md:100, 258 | the process Distilled Context describes |
| Stub draft | CONTEXT.md:136 | 'a stub draft' for fix Jobs |
| Hold | CONTEXT.md:136 | 'a draft holds the open Job' and 'waits for the draft' (152) |
| Adopted | none in the part | docs/contract.md:3734 says 'adopted PR': undefined in the glossary; no use in the part |
| Park, parked | none in the part | `rg -w -i 'park\|parked'` finds none in CONTEXT.md or the audited files |
| Archived | CONTEXT.md:104, 212 | Plan and Session archive state, no entry |
| Owner | CONTEXT.md:264 | GitHub owner; only Known owner is an entry |

## Avoid lists

(a) Avoid words that another file of the part (or CONTEXT.md outside the Avoid lines) uses as a live term. Count = lines found by `rg -n -w -i -- '<word>' <file>` with CONTEXT.md's `_Avoid_` lines removed; the docs/implementing.md count covers the whole file by the same `rg`, including the unread region.

| Avoid word | Under entry | Live use | Count | Verdict |
|---|---|---|---|---|
| epic | Context (entity) CONTEXT.md:89 | CONTEXT.md:9, README.md:11 | CONTEXT 4, README 1 | the levels table names levels by Jira words: caption it |
| task | Plan CONTEXT.md:109, Job CONTEXT.md:117, Inbox item CONTEXT.md:175 | CONTEXT.md:11, README.md:13, CONTEXT.md:108 'Jira task' | CONTEXT 2, README 1 | same |
| ticket | Spec CONTEXT.md:133, Brief CONTEXT.md:209 | CONTEXT.md:3, CONTEXT.md:10, README.md:5, README.md:12 | CONTEXT 3, README 2 | same |
| workflow | Plan CONTEXT.md:109, Engine CONTEXT.md:183 | docs/cockpit-parity.md (9 lines), BACKLOG.md:16 | cockpit-parity 9, BACKLOG 1 | becomes a live term |
| step | Job CONTEXT.md:117 | Deploy steps in CONTEXT.md; docs/implementing.md:18 'engine steps' | CONTEXT 11, implementing 28 | live term |
| message | Event CONTEXT.md:167 | CONTEXT.md:216 'The message the pi adapter adds' | CONTEXT 5, implementing 18 | real |
| owner | User CONTEXT.md:201 | CONTEXT.md:264 and 11 more lines | CONTEXT 12, implementing 2 | real GitHub term |
| done | Merged CONTEXT.md:249 | CONTEXT.md:15 'done' is a phase | CONTEXT 6 | real: phase `done` |
| level | Tier CONTEXT.md:253 | CONTEXT.md:7 'Level', CONTEXT.md:236 'PR level' | CONTEXT 5, README 1, implementing 10 | real |
| checks | Gate CONTEXT.md:237 | CONTEXT.md:68 'Mechanical checks' | CONTEXT 7, implementing 10, cockpit-parity 5 | real |
| CI | Gate CONTEXT.md:237 | CONTEXT.md:236, CONTEXT.md:75 | CONTEXT 5, implementing 3, BACKLOG 1 | live, and stale |
| run | Plan CONTEXT.md:109, Attempt CONTEXT.md:145 | `run/<plan>/` CONTEXT.md:136; `attempt run` docs/implementing.md:45 | CONTEXT 6, implementing 107 | real |
| retry | Attempt CONTEXT.md:145 | `job retry` docs/implementing.md:39 | implementing 6 | real command |
| ask | Escalation CONTEXT.md:231 | `cpd ask` docs/implementing.md:7 | implementing 21 | real command |
| board | Ledger CONTEXT.md:149 | `board` docs/implementing.md:129 | implementing 1 | real module |
| thread | Worker CONTEXT.md:225 | CONTEXT.md:190 'review thread' | CONTEXT 1, implementing 14 | upstream term |
| config | Policy CONTEXT.md:265 | docs/implementing.md:32 `config` | implementing 40, CONTEXT 2 | real command |
| context | Policy key CONTEXT.md:285 | the Context entity, everywhere | CONTEXT 29 | the Avoid word is the name of another entry |
| scope | Policy key CONTEXT.md:285 | the Scope entry CONTEXT.md:271 | CONTEXT 8 | the Avoid word is the name of another entry |
| lock | Resource CONTEXT.md:289 | CONTEXT.md:60 `plan lock`; docs/implementing.md (31 lines) | CONTEXT 1, implementing 31, versioning 2 | real |
| install | Attach CONTEXT.md:311 | `cpd install` docs/implementing.md:423 | implementing 37, README 2 | real command |
| rule | Axiom CONTEXT.md:259 | docs/implementing.md:5 'The one-area rule' | implementing 15, BACKLOG 2 | live |
| brief | Spec CONTEXT.md:133 | the Brief entry CONTEXT.md:207 | CONTEXT 6 | the Avoid word is the name of another entry |

(b) Avoid words that are real cpd terms elsewhere (commands or modules): `retry` (`job retry`, docs/implementing.md:39), `ask` (`cpd ask`, docs/implementing.md:7 and :42), `board` (docs/implementing.md:129), `config` (docs/implementing.md:32), `install` (docs/implementing.md:423), `run` (`attempt run`, docs/implementing.md:45), `lock` (`plan lock`, CONTEXT.md:60). Proposal: an Avoid word replaces a noun, not a command; say so on the Avoid line, or drop the word. The User decides.

(c) Avoid words with no live use in the part (count 0 by `rg -w -i` in every file): discovery, intake, decomposition, wiki, folder, card, category, opener, blocker, prerequisite, tracker, proposed, notification(s), alert(s), dashboard, scheduler, operator, master, supervisor, orchestrator, WIP, coordinator, subagent, executor, completed, principle, philosophy, layer, visibility, permission, selector, semaphore, seat, plugin, track, slice.

## PR occurrences

Every token of the set `PR`, `PRs`, `pull request(s)`, `pr` (command, flag, module, key, tag), `prs`, `pr_*`, `pull_requests` in the part (docs/interface.md: Description column only; docs/adr: first paragraph only). One row per quote; Count is the number of tokens the quote carries; all rows are listed, so the `mixed`, `phase` and `step` rows are individually visible. Sense codes: `object`, `pull request`, `phase`, `step`, `mixed`, `other`. `other` is explained in the last column. A reviewer can check a row with `sed -n '<line>p' <path>`.

### Groups

| Where (file or section) | Count | object | pull request | phase | step | mixed | other |
|---|---|---|---|---|---|---|---|
| CONTEXT.md, Introduction (lines 1-11) | 1 | 0 | 1 | 0 | 0 | 0 | 0 |
| CONTEXT.md, Phases (lines 13-31) | 10 | 4 | 1 | 1 | 0 | 4 | 0 |
| CONTEXT.md, The CPD flow (lines 33-85) | 18 | 5 | 4 | 0 | 2 | 7 | 0 |
| CONTEXT.md, Work units (lines 87-157) | 28 | 20 | 3 | 1 | 0 | 4 | 0 |
| CONTEXT.md, Runtime (lines 159-195) | 28 | 3 | 24 | 0 | 0 | 1 | 0 |
| CONTEXT.md, Roles (lines 197-231) | 5 | 2 | 1 | 0 | 0 | 2 | 0 |
| CONTEXT.md, Outcomes (lines 233-253) | 12 | 6 | 1 | 0 | 0 | 5 | 0 |
| CONTEXT.md, Axioms and Policies (lines 255-323) | 5 | 2 | 1 | 0 | 0 | 2 | 0 |
| README.md | 2 | 0 | 2 | 0 | 0 | 0 | 0 |
| BACKLOG.md | 1 | 0 | 0 | 0 | 0 | 1 | 0 |
| docs/implementing.md | 65 | 25 | 11 | 0 | 0 | 18 | 11 |
| docs/versioning.md | 3 | 0 | 3 | 0 | 0 | 0 | 0 |
| docs/ui-preview.md | 3 | 2 | 0 | 0 | 0 | 1 | 0 |
| docs/cockpit-parity.md | 34 | 0 | 25 | 0 | 0 | 3 | 6 |
| docs/interface.md | 19 | 10 | 7 | 0 | 0 | 2 | 0 |
| docs/adr/0005-cpd-cli-control-plane.md | 1 | 0 | 1 | 0 | 0 | 0 | 0 |
| docs/adr/0006-rebase-trunk-based.md | 1 | 0 | 1 | 0 | 0 | 0 | 0 |
| docs/adr/0009-cockpit-replaces-herdr.md | 1 | 0 | 1 | 0 | 0 | 0 | 0 |
| docs/adr/0011-contexts-and-plans.md | 1 | 0 | 0 | 0 | 0 | 1 | 0 |
| Total | 238 | 79 | 87 | 2 | 2 | 51 | 17 |

### Rows

| Where (path:line) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| CONTEXT.md:3 | 1 | pull request | turns one human ticket into a reviewed pull request | PR (unchanged) |
| CONTEXT.md:15 | 2 | object | when a PR is added (`cpd pr add`) | Workflow |
| CONTEXT.md:26 | 1 | object | in which PRs start, their Jobs are executed | Workflow |
| CONTEXT.md:26 | 1 | mixed | each PR is opened by its open Job | decide: the Workflow is not opened; its pull request is |
| CONTEXT.md:26 | 1 | mixed | it ends when every PR of the Plan is open | decide: `open` is a status of the object and of its pull request |
| CONTEXT.md:29 | 1 | phase | **PR** (phase): | delete: the phase is `deploy`; the phase is now `deploy`: the entry is stale |
| CONTEXT.md:30 | 1 | mixed | until all are merged | decide: open, then merged: status of the object, event of its pull request |
| CONTEXT.md:30 | 1 | pull request | PR feedback becomes appended Jobs | PR (unchanged) |
| CONTEXT.md:30 | 1 | object | a PR added later (a follow-up | Workflow |
| CONTEXT.md:30 | 1 | mixed | it is done again when that PR merges | decide: the pull request merges; the Workflow becomes `merged` |
| CONTEXT.md:54 | 2 | object | Splitting the work into PRs and Jobs (`pr add`, `job add`) | Workflow |
| CONTEXT.md:57 | 1 | object | Ordering with `--needs` and PR needs | Workflow |
| CONTEXT.md:66 | 1 | step | Mechanical checks, PR, Reviewing and Monitoring, Addressing | PR (unchanged), step name; names the step `PR` (the pull request is opened) |
| CONTEXT.md:69 | 1 | object | when the PR's `pr.gate` is `local` | Workflow |
| CONTEXT.md:69 | 1 | mixed | `pr.gate` is `local` | decide: key `pr.gate`: the Gate runs per Workflow before its pull request opens; also stale, the contract names it `gate.pr` or `gate.merge` |
| CONTEXT.md:69 | 1 | mixed | `pr open` is the repair | decide: acts on a Workflow and opens its pull request |
| CONTEXT.md:71 | 1 | step | **PR** (step): | PR (unchanged), step name; the 'PR' step of Deploy: the pull request is opened |
| CONTEXT.md:72 | 1 | pull request | The pull request is opened | PR (unchanged) |
| CONTEXT.md:72 | 1 | object | by the PR's open Job's merge | Workflow |
| CONTEXT.md:72 | 1 | mixed | (`pr open` is the | decide: same as line 69 |
| CONTEXT.md:75 | 2 | pull request | review the open pull request; the engine polls it (`pr-feedback`) | PR (unchanged) |
| CONTEXT.md:79 | 1 | mixed | appended to the open PR (only `work` and review | decide: `open` is the Workflow's status and its pull request's; Jobs belong to the Workflow, the push reaches the pull request |
| CONTEXT.md:79 | 1 | pull request | pushes the Lead Branch to the pull request | PR (unchanged) |
| CONTEXT.md:79 | 1 | mixed | no update Job, no PR Gate | decide: the Gate (`gate.pr`) runs on the Workflow's Lead Branch before the pull request opens |
| CONTEXT.md:79 | 2 | mixed | `pr push` repairs a failed push; `pr rebase` is | decide: `pr push` and `pr rebase` act on a Workflow and push to its pull request |
| CONTEXT.md:108 | 1 | object | One deliverable over one or more PRs | Workflow |
| CONTEXT.md:111 | 1 | object | **PR** (entity, new: R34): | Workflow |
| CONTEXT.md:112 | 2 | object | A PR may need other PRs of the same Plan | Workflow |
| CONTEXT.md:112 | 1 | object | one repository may hold several PRs of one Plan | Workflow |
| CONTEXT.md:112 | 1 | pull request | the Pull Request opened from that branch | PR (unchanged) |
| CONTEXT.md:112 | 1 | phase | Distinct from the PR phase | delete: the phase is `deploy`; stale: the phase is now `deploy` |
| CONTEXT.md:116 | 1 | object | It belongs to exactly one PR of its Plan | Workflow |
| CONTEXT.md:120 | 1 | object | it needs an `impl` Job of its PR | Workflow |
| CONTEXT.md:120 | 1 | object | a `tdd` or `free` PR allows it | Workflow |
| CONTEXT.md:120 | 2 | object | A PR's shape (`pr add --shape tdd\|docs\|config` | Workflow |
| CONTEXT.md:120 | 1 | object | before its PRs can be `tdd` | Workflow |
| CONTEXT.md:124 | 1 | object | The last Job of every started PR | Workflow |
| CONTEXT.md:124 | 1 | object | every other Job of its PR is merged or cancelled | Workflow |
| CONTEXT.md:124 | 1 | object | merges the PR's base into the Lead Branch | Workflow |
| CONTEXT.md:124 | 1 | pull request | opens the pull request, ready, never a draft | PR (unchanged) |
| CONTEXT.md:124 | 1 | mixed | `pr open` is kept as the repair | decide: acts on a Workflow, opens its pull request |
| CONTEXT.md:128 | 1 | pull request | for a pull request that is already open | PR (unchanged) |
| CONTEXT.md:128 | 2 | mixed | `pr rebase` and `pr push` are the repair | decide: act on a Workflow, push to its pull request |
| CONTEXT.md:136 | 1 | object | every Job of its PR that exists when it is added | Workflow |
| CONTEXT.md:136 | 1 | object | before the PR's open Job runs | Workflow |
| CONTEXT.md:140 | 1 | object | another Job of the same PR is Merged | Workflow |
| CONTEXT.md:140 | 2 | object | Order across PRs is the PRs' needs | Workflow |
| CONTEXT.md:152 | 1 | mixed | A PR with a draft never opens by itself | decide: the Workflow does not open; its open Job opens the pull request |
| CONTEXT.md:156 | 1 | object | its title, Tier, PR, kind and Dependencies | Workflow |
| CONTEXT.md:174 | 1 | pull request | a pull request of the User | PR (unchanged) |
| CONTEXT.md:182 | 1 | pull request | polls Pull Requests | PR (unchanged) |
| CONTEXT.md:182 | 2 | object | starts a PR once the PRs it needs are merged | Workflow |
| CONTEXT.md:185 | 1 | pull request | **PR poll** (new, hs-d565): | PR (unchanged) |
| CONTEXT.md:186 | 1 | pull request | every open pull request of the User on GitHub | PR (unchanged) |
| CONTEXT.md:186 | 1 | pull request | table `pull_requests` | PR (unchanged) |
| CONTEXT.md:186 | 1 | pull request | `pr-feedback` only to the Lead whose PR owns | PR (unchanged) |
| CONTEXT.md:186 | 1 | object | the Lead whose PR owns the pull request | Workflow |
| CONTEXT.md:186 | 1 | pull request | owns the pull request that needs attention | PR (unchanged) |
| CONTEXT.md:186 | 1 | pull request | A pull request a search misses is read directly | PR (unchanged) |
| CONTEXT.md:186 | 2 | pull request | (`cpd pr poll`), and reads the detail with `cpd pr | PR (unchanged) |
| CONTEXT.md:187 | 2 | pull request | PR watcher, PR sync | PR (unchanged) |
| CONTEXT.md:190 | 1 | pull request | a pull request a Plan owns | PR (unchanged) |
| CONTEXT.md:190 | 1 | pull request | answered with a `cpd pr reply` | PR (unchanged) |
| CONTEXT.md:190 | 1 | pull request | The PR poll stores them | PR (unchanged) |
| CONTEXT.md:190 | 1 | pull request | (`pr-feedback`, reason `comments`) | PR (unchanged) |
| CONTEXT.md:193 | 1 | pull request | **PR post** (new, pro-d64, | PR (unchanged) |
| CONTEXT.md:194 | 1 | pull request | One message announcing a PR for review in Slack | PR (unchanged) |
| CONTEXT.md:194 | 1 | pull request | `cpd pr post` when the PR is ready | PR (unchanged) |
| CONTEXT.md:194 | 1 | pull request | when the PR is ready (open, green CI | PR (unchanged) |
| CONTEXT.md:194 | 1 | pull request | one post per PR | PR (unchanged) |
| CONTEXT.md:194 | 1 | pull request | the PR merges when checks pass unless it is stacked | PR (unchanged) |
| CONTEXT.md:194 | 1 | mixed | (base PR not yet merged) | decide: the base is another Workflow's Lead Branch or its pull request: cannot tell |
| CONTEXT.md:194 | 1 | pull request | `docs/contract.md` §2.2 "PR post" | PR (unchanged) |
| CONTEXT.md:195 | 1 | pull request | Slack announcement, PR announcement | PR (unchanged) |
| CONTEXT.md:204 | 1 | mixed | phase, PR opened or merged, Job failed, done | decide: Events `pr-opened`, `pr-merged`: keyed on the Workflow, about its pull request |
| CONTEXT.md:212 | 1 | object | each PR not merged and not cancelled | Workflow |
| CONTEXT.md:212 | 1 | object | not yet on the PR's base | Workflow |
| CONTEXT.md:216 | 1 | mixed | phase, PR opened or merged, Job failed, done | decide: same as line 204 |
| CONTEXT.md:227 | 1 | pull request | The review is the Pull Request's own, on GitHub | PR (unchanged) |
| CONTEXT.md:236 | 1 | mixed | run by cpd only at the PR level | decide: level of the Workflow (its Lead Branch) before the pull request opens |
| CONTEXT.md:236 | 1 | object | on a PR's Lead Branch, in a gate worktree | Workflow |
| CONTEXT.md:236 | 1 | object | at the submit of the PR's open or update Job | Workflow |
| CONTEXT.md:236 | 3 | mixed | in `pr open`, `pr push` and `pr rebase`, the repair | decide: act on a Workflow, push to its pull request |
| CONTEXT.md:236 | 1 | object | only when the PR's Policy sets `pr.gate = "local"` | Workflow |
| CONTEXT.md:236 | 1 | mixed | `pr.gate = "local"` | decide: stale key, see line 69 |
| CONTEXT.md:244 | 2 | object | The branch of one PR that accumulates its Merged Jobs | Workflow |
| CONTEXT.md:244 | 1 | pull request | becomes its Pull Request | PR (unchanged) |
| CONTEXT.md:248 | 1 | object | rebased cleanly onto its PR's Lead Branch | Workflow |
| CONTEXT.md:268 | 2 | mixed | One typed slot of a Policy, owned by cpd's code: `pr`, | decide: Domain `pr` and key `pr.gate` hold pull request settings and the Workflow's Gate |
| CONTEXT.md:280 | 1 | pull request | the table `pr.post` is set in owner and repo files only | PR (unchanged) |
| CONTEXT.md:284 | 2 | object | a PR, its Workers and the commands on it use the PR's repo | Workflow |
| README.md:5 | 1 | pull request | turns one human ticket into a reviewed pull request | PR (unchanged) |
| README.md:20 | 1 | pull request | opens and tracks the pull request | PR (unchanged) |
| BACKLOG.md:16 | 1 | mixed | Estimated 7 PRs: | decide: 7 Workflows planned or 7 pull requests: cannot tell |
| docs/implementing.md:46 | 3 | mixed | \| `pr` \| rs-pr \| `pr …` (§9) | decide: module `pr` holds Workflow commands (add, edit, cancel, start, tables) and pull request commands (poll, post, merge) |
| docs/implementing.md:46 | 2 | mixed | `pr open`, `pr poll`, `pr rebase`, and since R34 `pr add` | decide: `pr open` and `pr rebase`: a Workflow and its pull request |
| docs/implementing.md:46 | 1 | pull request | `pr poll`, `pr rebase` | PR (unchanged) |
| docs/implementing.md:46 | 4 | object | `pr add`, `pr edit`, `pr cancel`, `pr start` (§9.4–§9.7) | Workflow |
| docs/implementing.md:46 | 2 | object | owns the tables `prs` and `pr_needs` | Workflow |
| docs/implementing.md:46 | 2 | object | the PR selection by `--pr` (§1.3) | Workflow |
| docs/implementing.md:46 | 1 | object | the PR start (contract §3.8, §7.2) | Workflow |
| docs/implementing.md:99 | 4 | pull request | Only a breaking migration the PR itself adds | PR (unchanged) |
| docs/implementing.md:111 | 1 | pull request | (`pr poll`, `attempt run`, `session reload` | PR (unchanged) |
| docs/implementing.md:120 | 1 | other | `cli/tests/pr/git_guard.rs` | test binary and directory name `pr` |
| docs/implementing.md:127 | 2 | other | \| `pr` \| `pr*`, `merge`, `git*`, `repo_hooks` \| | test area `pr` and its file glob `pr*` |
| docs/implementing.md:136 | 1 | object | until PR final | Workflow |
| docs/implementing.md:156 | 1 | object | (PRs port-run, port-tmux, ports) | Workflow |
| docs/implementing.md:158 | 1 | object | Timers and which PR gives each the seam | Workflow |
| docs/implementing.md:160 | 1 | object | \| Timer \| PR \| | Workflow |
| docs/implementing.md:162 | 1 | mixed | \| this PR (`Store::open_with`) \| | decide: the work unit and the pull request that carries it |
| docs/implementing.md:168 | 1 | mixed | PR rebase guard | decide: guard of `pr rebase` |
| docs/implementing.md:170 | 2 | mixed | Do not describe code that this PR has not created yet | decide: same as line 162 |
| docs/implementing.md:211 | 1 | other | `pr/gh.rs run_timed` (29 callers) | source path of the `pr` area (gh calls) |
| docs/implementing.md:214 | 1 | other | `watch::pr` | engine module `pr` (E5, polls pull requests) |
| docs/implementing.md:216 | 1 | object | PRs port-tmux and port-git fill it | Workflow |
| docs/implementing.md:221 | 2 | object | (Plan tests-mock, PR `mx-pure`) | Workflow |
| docs/implementing.md:231 | 1 | object | The tag is a PR key of Plan tests-mock | Workflow |
| docs/implementing.md:231 | 2 | other | m-flow, m-pr-a, m-pr-b, m-tooling | tag names (Plan tests-mock work units) |
| docs/implementing.md:231 | 2 | mixed | when a PR lands, its migration Jobs delete its lines | decide: `lands` = its pull request merges; its Jobs belong to the Workflow |
| docs/implementing.md:232 | 2 | other | m-flow, m-pr-a, m-pr-b, m-tooling, contract, m-pi | tag names |
| docs/implementing.md:234 | 2 | other | `pr::*` | engine step module `pr` |
| docs/implementing.md:234 | 1 | object | `Fx::with_plan` adds Plan `pay` with PR `repo` | Workflow |
| docs/implementing.md:234 | 1 | pull request | `cpd pr poll` through `gh` | PR (unchanged) |
| docs/implementing.md:242 | 2 | object | Decided API (PR ports2) | Workflow |
| docs/implementing.md:290 | 2 | pull request | runs `<exe> pr poll` through the runner | PR (unchanged) |
| docs/implementing.md:291 | 1 | mixed | or `pr rebase` is a `FakeRunner` reply | decide: a Workflow and its pull request |
| docs/implementing.md:315 | 1 | object | removed by the last PR of Plan tests-mock | Workflow |
| docs/implementing.md:333 | 1 | object | in the test files of PR port-git-fixtures-tests | Workflow |
| docs/implementing.md:337 | 1 | object | PR testkit-hosts puts the fixtures of the two hosts | Workflow |
| docs/implementing.md:358 | 2 | object | PR testkit2 adds, on top of `Fx` (PR tk0) | Workflow |
| docs/implementing.md:366 | 1 | mixed | **Not in this PR.** | decide: the work unit and its pull request |
| docs/implementing.md:430 | 2 | pull request | before the PR merges | PR (unchanged) |
| docs/implementing.md:604 | 1 | mixed | The PR follows the TDD order | decide: shape `tdd` of the object; also the pull request |
| docs/implementing.md:623 | 1 | mixed | One PR per upstream release, its title the release | decide: one Workflow or one pull request: title and body are the pull request's |
| docs/implementing.md:631 | 1 | mixed | The PR follows the TDD order | decide: same as line 604 |
| docs/implementing.md:656 | 2 | mixed | One PR per upstream release, its title the release | decide: same as line 623 |
| docs/versioning.md:20 | 2 | pull request | Two PRs that each add a migration | PR (unchanged) |
| docs/versioning.md:36 | 1 | pull request | The commit subjects are the PR titles plus | PR (unchanged) |
| docs/ui-preview.md:9 | 1 | object | The Lead adds a throwaway PR to the Plan | Workflow |
| docs/ui-preview.md:9 | 1 | mixed | The PR is never opened; it is cancelled | decide: never opened: its pull request; cancelled: the object's status |
| docs/ui-preview.md:11 | 1 | object | From the PR's Lead worktree, run: | Workflow |
| docs/cockpit-parity.md:9 | 1 | pull request | current source draws only the PR there | PR (unchanged) |
| docs/cockpit-parity.md:14 | 1 | pull request | "PRs and their feedback, the engine's state" | PR (unchanged) |
| docs/cockpit-parity.md:14 | 1 | pull request | PRs listed by Plan label | PR (unchanged) |
| docs/cockpit-parity.md:34 | 1 | pull request | cpd 2 adds a PR line | PR (unchanged) |
| docs/cockpit-parity.md:46 | 2 | pull request | homeserver, sep, PRs (`Min`), sep, git | PR (unchanged) |
| docs/cockpit-parity.md:53 | 1 | pull request | \| ` PRs` title \| | PR (unchanged) |
| docs/cockpit-parity.md:54 | 1 | pull request | \| A Plan's PR, line 1 \| | PR (unchanged) |
| docs/cockpit-parity.md:55 | 3 | pull request | \| A Plan's PR, line 2 \| | PR (unchanged) |
| docs/cockpit-parity.md:56 | 1 | pull request | `pr_state`, `checks` | PR (unchanged) |
| docs/cockpit-parity.md:57 | 3 | mixed | \| A Plan without a PR \| | decide: no pull request opened yet, or no object: cannot tell |
| docs/cockpit-parity.md:64 | 1 | pull request | every 2 s; PRs via `gh` with a 60 s cache | PR (unchanged) |
| docs/cockpit-parity.md:64 | 1 | pull request | changes of the pull request rows | PR (unchanged) |
| docs/cockpit-parity.md:181 | 1 | pull request | hostname with load, PRs, git (future) | PR (unchanged) |
| docs/cockpit-parity.md:182 | 1 | pull request | with their PR (number, state, checks | PR (unchanged) |
| docs/cockpit-parity.md:182 | 1 | pull request | `pr-feedback`, `plan-done` | PR (unchanged) |
| docs/cockpit-parity.md:186 | 1 | pull request | `pr poll` stores the PR's `title` | PR (unchanged) |
| docs/cockpit-parity.md:186 | 1 | pull request | stores the PR's `title`, `review_decision` | PR (unchanged) |
| docs/cockpit-parity.md:193 | 1 | pull request | \| PR state \| `gh pr view` every 60 s per PR, cached | PR (unchanged) |
| docs/cockpit-parity.md:193 | 1 | other | `gh pr view` | upstream GitHub CLI command; stays |
| docs/cockpit-parity.md:193 | 1 | pull request | every 60 s per PR, cached | PR (unchanged) |
| docs/cockpit-parity.md:193 | 1 | pull request | written by `pr poll` (E5) | PR (unchanged) |
| docs/cockpit-parity.md:202 | 1 | other | `parity-pr-data` and `parity-needs-you` | Job key of this historical audit |
| docs/cockpit-parity.md:210 | 3 | other | `parity-pr-data` \| `pr poll` stores PR title | Job key `parity-pr-data`, paths `cli/src/pr/` and `cli/tests/pr.rs` |
| docs/cockpit-parity.md:210 | 2 | pull request | `pr poll` stores PR title, review decision and checks | PR (unchanged) |
| docs/cockpit-parity.md:212 | 1 | other | `parity-dispatch` \| The Dispatch column as the reference \| S3 \| `parity-pr-data` | Job key |
| docs/cockpit-parity.md:212 | 1 | pull request | the PR rows match the capture's format | PR (unchanged) |
| docs/interface.md:41 | 1 | pull request | Cancel the Plan's Jobs and pull requests | PR (unchanged) |
| docs/interface.md:65 | 1 | mixed | Pull requests (commands.md §9) | decide: group `cpd pr` holds Workflow and pull request commands |
| docs/interface.md:66 | 1 | object | Add a PR to the Plan (§9.4) | Workflow |
| docs/interface.md:67 | 1 | object | Change a waiting PR before the lock (§9.5) | Workflow |
| docs/interface.md:68 | 1 | object | Store the PR's description | Workflow |
| docs/interface.md:68 | 1 | pull request | which becomes the pull request body | PR (unchanged) |
| docs/interface.md:69 | 3 | object | Remove a PR from the Plan before the lock | Workflow |
| docs/interface.md:70 | 1 | object | Start a waiting PR whose needs are merged | Workflow |
| docs/interface.md:71 | 1 | mixed | Push the Lead Branch and open the PR, refused only | decide: the Workflow's Lead Branch, the pull request opened |
| docs/interface.md:72 | 1 | pull request | Poll every open pull request of the User | PR (unchanged) |
| docs/interface.md:75 | 1 | pull request | Show the PR's checks, reviews and comments | PR (unchanged) |
| docs/interface.md:76 | 1 | object | feedback item of the PR's pull request | Workflow |
| docs/interface.md:76 | 1 | pull request | of the PR's pull request as the User | PR (unchanged) |
| docs/interface.md:77 | 1 | object | Post the PR's pull request to Slack | Workflow |
| docs/interface.md:77 | 1 | pull request | Post the PR's pull request to Slack for review | PR (unchanged) |
| docs/interface.md:78 | 1 | pull request | Squash-merge a clean pull request on GitHub | PR (unchanged) |
| docs/interface.md:169 | 1 | object | merged and cancelled PRs (commands.md §29) | Workflow |
| docs/adr/0005-cpd-cli-control-plane.md:7 | 1 | pull request | merge queue, Gate, metrics, PR polling | PR (unchanged) |
| docs/adr/0006-rebase-trunk-based.md:7 | 1 | pull request | the PR is rebased onto the default branch only | PR (unchanged) |
| docs/adr/0009-cockpit-replaces-herdr.md:8 | 1 | pull request | the right is future work: PRs, git, Jira | PR (unchanged) |
| docs/adr/0011-contexts-and-plans.md:7 | 1 | mixed | one repo and one PR for now | decide: birth of the object: a Plan was one PR |

## Fix list

Only what needs a judgement is a row; the mechanical rewrite is a rule. Quotes in the second column are found on the cited line.

RULES (mechanical, no row each; the counts are `object` tokens from `## PR occurrences`):

- R1. `object` PR becomes Workflow in CONTEXT.md: 42 tokens (Introduction 0; Phases 4; The CPD flow 5; Work units 20; Runtime 3; Roles 2; Outcomes 6; Axioms and Policies 2).
- R2. `object` PR becomes Workflow in docs/implementing.md: 25 tokens, of which 16 are keys of Plan tests-mock work units (for example `PR ports2`, `PR testkit2`) and 9 are `pr` command or table words (line 46).
- R3. `object` PR becomes Workflow in docs/ui-preview.md: 2 tokens; docs/interface.md: 10 Description tokens (fix the clap help text, then regenerate with `CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface`).
- R4. `pull request` tokens (87) stay as they are; 'PR' keeps this meaning. No change in docs/cockpit-parity.md, docs/versioning.md, README.md.
- R5. docs/adr: no change, history (4 tokens, listed above).

| path:line | what to change | why |
|---|---|---|
| CONTEXT.md:3 | Add an exception to "means what it meant there": Workflow returns with a new meaning | Workflow is retired in cpd 1 terms and live again; the sentence would be false |
| CONTEXT.md:5 | Add the Workflow level to the levels table (Plan > Workflows > Jobs) under "The levels, the cpd objects and their roles" | the table shows three levels; the object between Plan and Job has none |
| README.md:7 | Replace the copy of the levels table (lines 9-13) by a pointer to CONTEXT.md; keep "The levels, the cpd objects and their roles:" out of the README | one source of truth: README restates the glossary table |
| CONTEXT.md:26 | mixed: "each PR is opened by its open Job" becomes the pull request of each Workflow; "it ends when every PR of the Plan is open" becomes every Workflow's pull request is open | the Workflow is not opened, its pull request is |
| CONTEXT.md:29 | Delete the entry "**PR** (phase):" and keep its follow-up rule (a later Workflow moves a done Plan back to dispatch) in the Deploy stage or the phase list | phase: the phase is now `deploy` (CONTEXT.md:15); the entry is stale |
| CONTEXT.md:30 | mixed: "until all are merged" and "it is done again when that PR merges": say the pull request merges | object status and pull request event are two things |
| CONTEXT.md:36 | Check "the cockpit's three columns": the stages are Context, Plan, Deploy, a column is titled Dispatch (docs/cockpit-parity.md:65) | unverified in the running cockpit; names must agree |
| CONTEXT.md:66 | step: keep the step name PR (it now means the pull request) or rename it, in the Deploy list "Mechanical checks, PR, Reviewing and Monitoring, Addressing" | after the rename PR is unambiguous; a step with a one-word name PR still reads like the object. The User decides |
| CONTEXT.md:69 | stale key: "the PR's `pr.gate` is `local`" becomes the contract's `gate.merge` or `gate.pr` (docs/contract.md:3963) | the key `pr.gate` no longer exists (contract §8.3, hs-d679) |
| CONTEXT.md:69 | mixed (also CONTEXT.md:72, 79, 124, 128, 236 and docs/implementing.md:46, 291): the repairs "`pr open` is the repair", `pr push`, `pr rebase` | each acts on a Workflow and reaches its pull request. Proposal: commands that act on the pull request (open, push, rebase, poll, view, reply, post, merge) keep `pr`; commands that add, edit, cancel or start the object become `workflow`; the flag `--pr <key>` becomes `--workflow <key>`. The User decides; open, push, rebase are the contested three |
| CONTEXT.md:79 | mixed: "no update Job, no PR Gate" becomes the `gate.pr` Gate; "appended to the open PR" says which one | the Gate runs on the Workflow's Lead Branch before the pull request opens |
| CONTEXT.md:104 | Delete the entry Grounded Lead or move it to history | stale: "No longer created (B3)"; ADR-0011 holds the history |
| CONTEXT.md:100 | Rename or split Distilled Context: it describes the process "The curator at work"; shorten to a pointer to the contract | a Context term that names a process; restates contract §4.6 (unverified) |
| CONTEXT.md:108 | Rewrite the Plan entity: delete "the term Workflow is retired" and 'replaces Workflow' (107), and the Avoid word `workflow` (109) | Workflow returns with the new meaning |
| CONTEXT.md:112 | Rename the entry PR (entity) to Workflow; delete "Distinct from the PR phase" | object renamed; phase deleted |
| CONTEXT.md:112 | Lowercase "the Pull Request opened from that branch"; same at CONTEXT.md:182, 227, 244 | inconsistent capitals: lowercase everywhere else |
| CONTEXT.md:120 | Add a Shape entry (tdd, docs, config, free) for "A PR's shape" | used, never defined |
| CONTEXT.md:152 | mixed: "A PR with a draft never opens by itself" becomes a Workflow whose open Job waits | the pull request opens, not the Workflow |
| CONTEXT.md:183 | Avoid word "watcher per workflow" (Engine): reword | Workflow is now a live term |
| CONTEXT.md:186 | Delete "feeds the Dispatch column" | commit 32bb8cd3 removed the PRs section from the widgets column (commit message; code not run) |
| CONTEXT.md:190 | Cut the entry to a pointer: it says "Defined once in `docs/contract.md` §2.2"; same for PR post (CONTEXT.md:194) | restates the contract; DRY |
| CONTEXT.md:194 | mixed: "(base PR not yet merged)": add a Stacked entry that names the base | a base Workflow or a base pull request: the text does not say |
| CONTEXT.md:204 | mixed: "PR opened or merged" (also CONTEXT.md:216) | Event kinds pr-opened and pr-merged (core/src/events/kind.rs:104, 105) are keyed on a Workflow and tell of its pull request; core/src/events/kind.rs:98-103 are object-only |
| CONTEXT.md:227 | Delete the entry Reviewer (removed) | stale: "Removed by R19"; history is docs/adr/0002-review-before-merge.md:1 |
| CONTEXT.md:236 | mixed: "run by cpd only at the PR level" becomes the Workflow level; the key "sets `pr.gate" is stale, see CONTEXT.md:69 | level of the Workflow; stale key |
| CONTEXT.md:237 | Avoid word "CI, checks" under Gate: keep only CI; "checks" is GitHub's word and cpd's Mechanical checks | the entry says "the repo's CI is the check" at line 236 and CONTEXT.md:68 defines Mechanical checks |
| CONTEXT.md:240 | Resolve the contradiction: "The repository's `pre-commit` hook" is called the Gate by docs/contract.md:3560 | two names for one hook in two documents |
| CONTEXT.md:268 | mixed: Domain `pr` and key `pr.gate`: keep Domain `pr` for pull request settings and send the Gate to the `gate` Domain | the contract already has the `gate` domain (docs/contract.md:3963) |
| CONTEXT.md:292 | Rewrite the Slot definition: "one concurrent unit of it" only | the entry defines Slot as a number and as one unit |
| CONTEXT.md:318 | Remove the names from Known owner; point to `cpd policy show`; same for Fallback (CONTEXT.md:322) | instance data in a vocabulary file goes stale |
| CONTEXT.md:277 | Remove the extra blank line before Exposure (277-278) | formatting |
| CONTEXT.md:109 | Avoid lists: say in each Avoid line that the word is the noun it replaces (retry, ask, board, config, install, run, lock are real commands or modules) | docs/implementing.md:7, :32, :39, :45, :129 and CONTEXT.md:60 use them |
| docs/implementing.md:46 | mixed: split the `pr` module row ("`pr …` (§9)") into Workflow commands and pull request commands; rename the tables `prs` and `pr_needs` with the Workflow | module `pr` owns both senses |
| docs/implementing.md:127 | other: the test area `pr` and its glob `pr*` follow the command rename | names of a test binary: the User decides |
| docs/implementing.md:162 | mixed: "this PR (`Store::open_with`)" (also :170, :366): say 'this change' | 'PR' here is the work unit and its pull request |
| docs/implementing.md:168 | mixed: "PR rebase guard" means the guard of `pr rebase` | command name, see CONTEXT.md:69 |
| docs/implementing.md:231 | other: tags "m-pr-a, m-pr-b" are identifiers in files; rename only with the Plan | identifiers, not prose; the User decides |
| docs/implementing.md:231 | mixed: "when a PR lands, its migration Jobs delete its lines" says Workflow merged | the Jobs belong to the Workflow |
| docs/implementing.md:413 | Delete "CI's `rust` job runs the same command", and 'the Gate and CI also run' (416), or say 'where CI exists' | no `.github` in the repo (verified); the repo policy says no GitHub Actions |
| docs/implementing.md:604 | mixed: "The PR follows the TDD order" (also :631); 'One PR per upstream release, its title the release' (:623, :656) | shape tdd belongs to the Workflow; title and body belong to the pull request |
| docs/versioning.md:5 | Replace "Today it is **13**" by the rule only | code has PROTOCOL 17 |
| docs/versioning.md:6 | Replace "Today it is **45**" by the rule only | code has 75 migration files |
| docs/ui-preview.md:9 | mixed: "The PR is never opened; it is cancelled after the preview." is a Workflow that never opens its pull request | status of the object, event of the pull request |
| BACKLOG.md:6 | Ask the User: "Move CI from `ubuntu-24.04` to `ubuntu-26.04`" is still wanted | no `.github` in the repo (verified); the repo policy says no GitHub Actions |
| BACKLOG.md:16 | Reword "Remote workflows" and mixed "Estimated 7 PRs" | Workflow is a live term: 'Remote workflows' would read as the new object |
| docs/cockpit-parity.md:1 | Move the file to docs/research or docs/archive, or leave it as history | "cpd 2 cockpit: parity checklist (an audit against the running cpd 1 cockpit)" uses the cpd 1 Workflow 11 times (lines 33 to 227) and the Dispatch column; history keeps old words |
| docs/cockpit-parity.md:57 | mixed: "\| A Plan without a PR \|" (also the `no PR` text on that line) | no pull request yet, or no Workflow: the text cannot tell |
| docs/interface.md:65 | mixed: fix in the clap help text under cli/src, not in the generated file: "Pull requests (commands.md §9)"; also line 71 | docs/interface.md is generated; the group `cpd pr` holds both senses |

## Counts

| Measure | Number |
|---|---|
| Files in the part | 9 (9 named files and docs/adr/*.md as one group of 14) |
| Lines read in full | 998 (CONTEXT.md 323, README.md 54, BACKLOG.md 16, docs/implementing.md 1-282, docs/versioning.md 45, docs/mark.md 13, docs/ui-preview.md 33, docs/cockpit-parity.md 232) |
| Lines skimmed | 404 (docs/implementing.md 283-686) and the docs/interface.md Description column |
| Entries in CONTEXT.md (`rg -n '^\*\*' CONTEXT.md \| wc -l`) | 78 |
| Rows in `## Entries` | 78 (equals the entries: yes) |
| `_Avoid_` lines in CONTEXT.md | 60 |
| Rows in `## Terms` | 70 |
| Rows in `## Same word, several senses` | 28 |
| Rows in `## Used but not defined` | 17 |
| Rows in `## Avoid lists` (a) | 23 |
| Rows in `## PR occurrences` (Rows table) | 188 |
| Rows in `## Fix list` | 48, plus 5 rules |
| PR tokens in all rows | 238 |
| Occurrences with sense `object` | 79 |
| Occurrences with sense `pull request` | 87 |
| Occurrences with sense `phase` | 2 |
| Occurrences with sense `step` | 2 |
| Occurrences with sense `mixed` | 51 |
| Occurrences with sense `other` | 17 |
| `pull request` tokens by file | CONTEXT.md 36, README.md 2, docs/implementing.md 11, docs/versioning.md 3, docs/cockpit-parity.md 25, docs/interface.md 7, docs/adr/0005-cpd-cli-control-plane.md 1, docs/adr/0006-rebase-trunk-based.md 1, docs/adr/0009-cockpit-replaces-herdr.md 1 |
| Capital `PR` or `PRs` (whole files, `rg -o -w`) | CONTEXT.md 66, docs/implementing.md 36, docs/cockpit-parity.md 22, docs/interface.md 12, docs/versioning.md 3, docs/ui-preview.md 3, BACKLOG.md 1 |
| Workflow (any case) in the part | CONTEXT.md 6, BACKLOG.md 1, docs/cockpit-parity.md 11, docs/adr first paragraphs 8 (0001: 3, 0003: 1, 0004: 1, 0005: 1, 0011: 2, counting `workflow preparation`) |
