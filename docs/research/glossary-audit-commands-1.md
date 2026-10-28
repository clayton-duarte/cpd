# Glossary audit: docs/commands.md sections 1 to 8 (conventions to ask)

Report only: no file but this one was changed. Part: `docs/commands.md` lines 1 to 1730, from the top through the line before `## 9. `pr``. Glossary: `CONTEXT.md`. Decisions behind the audit: pro-d136, pro-d138 (the cpd object "PR" becomes Workflow; "PR" keeps only the pull request). Line numbers are those of `docs/commands.md` at the base commit of this Attempt.

## Coverage

| Path | Lines | What | Read |
|---|---|---|---|
| docs/commands.md | 1-44 | title, status, Contents | in full |
| docs/commands.md | 45-217 | §1 Conventions (1.1 to 1.5), the error table of §1.3 | in full |
| docs/commands.md | 218-257 | §2 Command index | in full |
| docs/commands.md | 258-780 | §3 `context` (3.1 to 3.18) | in full |
| docs/commands.md | 781-1298 | §4 `plan` (4.1 to 4.14), with the `plan render` output and the `plan show` JSON | in full |
| docs/commands.md | 1299-1566 | §5 `job` (5.1 to 5.9) | in full |
| docs/commands.md | 1567-1674 | §6 `attempt` (6.1 to 6.4) | in full |
| docs/commands.md | 1675-1682 | §7 `review` (removed) | in full |
| docs/commands.md | 1683-1730 | §8 `ask` and `answer` | in full |

- **Read in full:** every line of 1 to 1730, with the `read` tool in six chunks (the last chunk ends at line 1731, the heading of §9, which is outside the part and not audited). I audited the vocabulary only: whether the specified behaviour is right was not checked.
- **Skimmed:** nothing. The long table cells of §1.3 and the single-line paragraphs of §3.13, §3.15, §4.5 and §5.1 were read whole, and each `PR` in them was located by line and column.
- **Not read:** everything outside lines 1 to 1730, including §9 onward (another part). `CONTEXT.md` (323 lines) was read in full to take the glossary's senses. `docs/contract.md` was not read: only `rg -n 'gate\.pr|pr\.gate' docs/contract.md docs/commands.md` was run, to check the Gate key spelling (used in one row; the contract's use is unverified beyond that grep). The `cpd` binary and code were not read: every statement about what the code does is the document's, unverified.
- **Method:** counts below are of the part only, taken from `head -1730 docs/commands.md`, so line 1731 onward is never counted. Each cited line and quote was checked by script against the file before the report was written.

| What | Command (run from the worktree) | Result |
|---|---|---|
| PR / PRs (capitalised, whole word) | `head -1730 docs/commands.md | rg -o '\bPRs?\b' | wc -l` | 305 |
| pull request(s), any case | `head -1730 docs/commands.md | rg -o -i 'pull requests?' | wc -l` | 28 |
| workflow / workflows (whole word, any case in the part) | `head -1730 docs/commands.md | rg -o -i -w 'workflows?' | wc -l` | 7 |
| `--pr` flag | `head -1730 docs/commands.md | rg -o -e '--pr\b' | wc -l` | 16 |
| error codes and other `pr_*` names | `head -1730 docs/commands.md | rg -o '\bpr_[a-z_]+' | wc -l` | 23 |
| `pr` command words | `head -1730 docs/commands.md | rg -o '\bpr (add|edit|cancel|start|open|push|rebase|poll|post|reply|merge|view)\b' | wc -l` | 76 |
| PR phase or PR step wording | `head -1730 docs/commands.md | rg -n -i 'PR phase|phase.{0,12}`?pr`?\b|PR step' | wc -l` | 1 |
| `pr_in_use`, `pr_locked` (named in the Spec) | `head -1730 docs/commands.md | rg -c 'pr_in_use|pr_locked'; echo exit:$?` | exit:1 |
| lines of the part | `head -1730 docs/commands.md | wc -l` | 1730 |
| first line of section 9 | `rg -n '^## 9\. ' docs/commands.md` | 1731:## 9. `pr` |

The `PR phase or PR step wording` hit is one false positive (L904, "every phase until the PR starts"): no use of `PR` as the old Plan phase exists in the part. The two names the Spec listed, `pr_in_use` and `pr_locked`, do not occur in the part (`rg -c` exit 1); the nearest is `owner_in_use` (L626), which carries `prs`.

## Terms

Verdicts: ok, loose, overloaded, stale, wrong. Each row cites every line it relies on; the quote is the first one.

| Term | Where (path:line) | Sense as used here | Collision (internal / upstream) | Verdict | Proposed wording |
|---|---|---|---|---|---|
| PR (step) | docs/commands.md:9 | `` (Mechanical checks, PR, Reviewing and Monitoring, Addressing) `` — step name: the pull request is opened | internal: PR entity (to be Workflow) and the retired PR phase (CONTEXT.md:29, :71) | stale | name the step by its act (for example "Opening"), as CONTEXT.md:71 is also to change; the User decides |
| PR (entity) | docs/commands.md:104 | `` `--pr <key>`, within the selected Plan `` — the cpd object (key, repo, base, Lead Branch, Jobs, status) | internal: abbreviation of pull request in the same file (`pr poll`, `pr post`, `pull_request`) | wrong | Workflow (decisions pro-d136, pro-d138); mechanical rule in the Fix list |
| workflow (cpd 1 sense) in "workflow worktree" | docs/commands.md:1076, docs/commands.md:1079, docs/commands.md:1580, docs/commands.md:1600 | `` its workflow worktree path does not exist `` — the PR's Lead worktree, named with cpd 1's word (workflow = Plan) | internal: after the rename Workflow is the new name of the PR object, so these four would read as correct by accident; upstream: GitHub Actions workflow | stale | Lead worktree (the term of L672 and CONTEXT.md:111) |
| workflow (cpd 1 history) | docs/commands.md:783, docs/commands.md:876, docs/commands.md:884 | `` Replaces cpd 1's `workflow` group (D3) `` — history: cpd 1's name for Plan, always with "cpd 1" | internal: the new Workflow (PR object) returns with this word | ok | keep; history keeps the old words. The "cpd 1" prefix is the safeguard |
| `pr` as a JSON key: Workflow key or pull request | docs/commands.md:837, docs/commands.md:856, docs/commands.md:926 | `` `pr` (`{number, url, opened_at}` or null) `` — in `plan list` and `plan show` `prs[].pr` is the pull request {number, url, opened_at}; in `jobs[].pr`, `overlaps[].pr` and `details` `{pr, …}` it is the PR key | internal: the same key `pr` has two meanings, and `pull_request` sits beside it | overloaded | rename `jobs[].pr` to `workflow`; name the other one `pull_request` only (drop `prs[].pr`) or `opened`; User decides (see PR occurrences) |
| A PR's title and Jira ticket | docs/commands.md:216, docs/commands.md:216, docs/commands.md:1002 | `` A PR's title (`--title` of `pr add` and `pr edit` `` — a title and a Jira key stored on the PR row that become the pull request's title and ticket | internal: title/ticket of the object or of the pull request; upstream: Jira ticket | overloaded | "Workflow title" if it is the Workflow's; "pull request title" if it is the pull request's; the code stores it on the row and uses it for the pull request, so propose "pull request title" |
| Plan (entity, stage, phase) | docs/commands.md:9, docs/commands.md:1345 | `` **Plan** (Dividing, Sequencing, Approving, Dispatching) `` — stage of the CPD flow (L9) and phase `plan` (L1345) and the entity | internal: CONTEXT.md:21 and :50 already name the Plan phase and the Plan stage apart from the entity | overloaded | keep; the doc says "phase" and "stage" each time. Risk only where "Plan" stands alone |
| Deploy (stage, phase) | docs/commands.md:9, docs/commands.md:1129 | `` and **Deploy** (Mechanical checks `` — stage (L9) and phase `deploy` (L1129) | internal: stage Deploy covers the end of phase `dispatch` too (CONTEXT.md:65) | overloaded | keep; say "phase `deploy`" or "the Deploy stage" each time |
| dispatch / Dispatching | docs/commands.md:9, docs/commands.md:1358, docs/commands.md:1460, docs/commands.md:1083 | `` Dispatching) and **Deploy** `` — step (L9), Plan phase `dispatch`, PR status `dispatch` (L1083, L1358) and the command `job dispatch` (L1460: start one Ready Job) | internal: four meanings; the PR status `dispatch` is not the Plan phase `dispatch` | overloaded | after the rename a Workflow status `dispatch` stays next to the phase `dispatch`; propose a different status word for the Workflow (User decides); name `job dispatch` "job start"? (contested, not proposed here) |
| waiting | docs/commands.md:542, docs/commands.md:1083, docs/commands.md:1701 | `` or `waiting` (hs-d523: display only `` — three states called `waiting`: a Job display status (L542), the PR status (L1083), a Session state (L1701) | internal: Session `waiting` means "waits for another agent" (CONTEXT.md:161); Job `waiting` is display only (hs-d523) and is not a Job status | overloaded | keep the Session and Workflow words; rename the Job display value (for example `held`); User decides |
| blocked | docs/commands.md:542, docs/commands.md:456, docs/commands.md:162 | `` display only, blocked by a dependency or PR that has not started `` — L542: a Job held by a Dependency; L456: Session state `blocked` (the User has to act); `archive_blocked`: a live object stops an archive | internal: Axiom "Blocked means it needs the human"; CONTEXT.md:161 defines `blocked` as the User must act | loose | L542: "waiting on" (as the same line already writes `waiting_on`); `archive_blocked` is an error code name, keep |
| open | docs/commands.md:123, docs/commands.md:908, docs/commands.md:456, docs/commands.md:189, docs/commands.md:104 | `` found the PR not `open` `` — PR status `open`, the `open` Job kind, an `open` Escalation, "open work", an open pull request | internal: five meanings; upstream: GitHub pull request state `OPEN` (L170) | overloaded | keep the identifiers; in prose write "open Job", "open pull request", "Workflow `open`" and replace "open work" (L908) with "unmerged work" |
| merged | docs/commands.md:542, docs/commands.md:658, docs/commands.md:146 | `` Here `merged` means on the Lead Branch and not yet on the base `` — Job `merged` = on the Lead Branch (L542); PR `merged` = its pull request merged (L658); GitHub `MERGED` | internal: Job Merged (CONTEXT.md:247) vs PR merged; upstream: git merge, GitHub squash merge | overloaded | keep; L542 already says which one. Say "Job merged" and "Workflow merged" where both occur |
| draft | docs/commands.md:130, docs/commands.md:837, docs/commands.md:1338 | `` `not_open`, `draft`, `conflicting` `` — Job `draft` (not approved) vs the GitHub pull request `draft` flag (L130, L837) | upstream: GitHub draft pull request; internal: Draft (CONTEXT.md:151) | overloaded | keep; "draft Job" vs "draft pull request" in prose |
| Locked / lock | docs/commands.md:904, docs/commands.md:166, docs/commands.md:212, docs/commands.md:1059 | `` the Jobs that are not `draft` are Locked context `` — Locked Job (state), "Locked context" (L904), file lock under `locks/`, the `plan lock` command | internal: "Locked context" reads as the Context entity; four meanings of lock; Resource Slot "Avoid: lock" (CONTEXT.md:287) | loose | L904: "Locked Jobs are shown as context" (lower-case context); keep the others |
| context (field, entity, stage) | docs/commands.md:1306, docs/commands.md:9 | `` "context": "…", "touches": ["docs/commands.md"] `` — Spec field `context` (L1306, :868, :1314), Context entity and the Context stage | internal: Spec field `context` vs Context (CONTEXT.md:87); the Spec's `context` is free text about the Job | overloaded | keep the field (stored, in the fingerprint); say "the Spec's `context` field" in prose |
| review | docs/commands.md:1340, docs/commands.md:996, docs/commands.md:837 | `` a **review Job** (B2, contract §2.2) `` — Plan review, review Job, GitHub review (`review_decision`) | upstream: GitHub review; internal: Plan review vs review Job vs the removed Reviewer | overloaded | keep; CONTEXT.md:135 and the code keep the three apart |
| Reviewer (section 7 tombstone) | docs/commands.md:1677, docs/commands.md:1675 | `` Removed by R19 (no Job-level Reviewer). `` — two headings kept for a removed command group | internal: Reviewer (removed), CONTEXT.md:227 | stale | keep the numbering (other files cite §8, §9); say so in the heading, or delete if no file cites §7 |
| check / checks | docs/commands.md:1604, docs/commands.md:837, docs/commands.md:1341 | `` **One check span** (contract §7.7, hs-d679) `` — check span, CI `checks` of the pull request, Quick check, Mechanical checks, tamper check | upstream: GitHub checks; internal: Gate "Avoid: checks" (CONTEXT.md:235) | overloaded | keep; name the CI one "pull request checks" |
| Gate (policy key `gate.pr`) | docs/commands.md:1600, docs/commands.md:1371 | `` `gate.pr` is `local` `` — policy key `gate.pr` (contract.md:3561 uses `gate.pr`, contract.md:3660 `gate.merge`) and "PR Gate" | internal: CONTEXT.md:69, :236 and :268 write `pr.gate`; "PR Gate" = Gate at the PR level | loose | one spelling across CONTEXT.md and contract (the contract's `gate.pr` is the one in code, unverified); "PR Gate" -> "Gate" with the level said once |
| session | docs/commands.md:818, docs/commands.md:1701 | `` the cockpit session `cpd` if it is missing `` — tmux session `cpd` (the cockpit) vs cpd Session | upstream: tmux session; internal: Session (CONTEXT.md:161) | overloaded | "tmux session" for the cockpit one (L818, L821) |
| slot | docs/commands.md:457, docs/commands.md:177, docs/commands.md:1478 | `` is in the chat slot `` — chat slot (L457), resource Slot (L177), Worker concurrency slot (`no_slot`, L1478) | internal: Slot (CONTEXT.md:291) = resource; chat slot is undefined there | overloaded | define "chat slot" in CONTEXT.md or write "the chat"; keep `no_slot` |
| Jira Epic / task / ticket | docs/commands.md:278, docs/commands.md:798, docs/commands.md:216 | `` Jira Epic key, stored only `` — Context = Epic (L278), Plan = "task" (L798), PR = "ticket" (L216) | upstream: Jira; internal: CONTEXT.md:9-11 maps Epic=Context, Ticket=Plan, Task=Job, so "Jira task" for a Plan contradicts the Level table (CONTEXT.md:108 says "Jira task" too) | overloaded | Plan: "Jira ticket" (the Level table); a Workflow has no Jira ticket in the Level table, so L216 and the `jira` field need a decision |
| outline | docs/commands.md:5, docs/commands.md:69, docs/commands.md:1200 | `` every command of outline §3 `` — L5 and L69: the cpd 2 design outline (a document); §4.13: the Plan outline (an object) | internal: two things called outline in one file | overloaded | "the design outline" (L5, L69) with its path; keep Plan outline |
| step | docs/commands.md:9, docs/commands.md:1172 | `` maps each step to its phase and commands `` — CPD-flow step (L9) vs numbered Effects steps ("step 5", "steps A1–A9") | internal: Job "Avoid: step" (CONTEXT.md:115) | loose | numbered Effects: "item 5"; keep "step" for the CPD flow only |
| state / status | docs/commands.md:170, docs/commands.md:1083, docs/commands.md:461 | `` the pull request's `state` is not `OPEN` `` — Workflow `status`, Job `status`, Session `state`, pull request `state` | upstream: GitHub `state`; internal: three names for one idea | loose | keep (code names); do not change |
| base (branch, PR, sha) | docs/commands.md:121 | `` the base branch of a stacked PR, its base PR's Lead Branch `` — `base` = base branch; `base_pr` = base PR; `base_sha` | internal: "base PR" will become "base Workflow"; `base` of a stacked PR is a Lead Branch | overloaded | after the rename write "base Workflow" (stacked); keep `base` for the branch |
| needs / Dependency | docs/commands.md:1337, docs/commands.md:169 | `` `--needs <k1,k2>` \| Dependencies, comma-separated `` — `--needs` of `job add` = Job Dependencies; `--needs` of `pr add` = PR needs | internal: Dependency (CONTEXT.md:139) "order across PRs is the PRs' needs" | loose | keep; one flag name, two objects; say "Job needs" and "Workflow needs" |
| decide / decision | docs/commands.md:372, docs/commands.md:1032, docs/commands.md:1218 | `` ### 3.5 `context decide`, `context note`, `context ask` `` — `context decide` writes a Context record; `plan decide` records the User's verdict on a Plan review; outline `decisions` | internal: one verb, three objects; Context record decision | overloaded | keep; `plan decide` could read "plan review decide" (contested, not proposed) |
| note | docs/commands.md:372, docs/commands.md:209 | `` ### 3.5 `context decide`, `context note`, `context ask` `` — Context record kind `note` vs the User's free-text note in the review dialog | internal: Context record | loose | "the User's feedback" (the file is `feedback-<review>.md`) |
| run | docs/commands.md:904, docs/commands.md:1661, docs/commands.md:1591 | `` writes `run/<plan>/plan.md` `` — directory `run/`, internal command `attempt run`, the `[test-commands]` `run` command | internal: Attempt "Avoid: run" (CONTEXT.md:143); `attempt run` and `session run` run the driver | loose | keep the identifiers; they are code names |
| post | docs/commands.md:126, docs/commands.md:773 | `` `pr post` (§9.8b) found a `pr_posts` row for the PR `` — PR post (Slack) vs board post (a message to a Session) | internal: PR post (CONTEXT.md:193) | loose | "board post" and "PR post" always with their qualifier |
| archive / asleep | docs/commands.md:656, docs/commands.md:658 | `` **archived or asleep**: the Context is archived `` — `context delete` and `plan delete` refuse with `archive_blocked` and use "asleep", which no glossary entry defines | internal: undefined word "asleep"; code `archive_blocked` used for deletes | wrong | define "asleep" in CONTEXT.md or say what it means (stopped, parked); a code `delete_blocked` is a code change, User decides |
| ready | docs/commands.md:1466, docs/commands.md:904 | `` Starts one Ready Job now `` — Ready Job (scheduler state) vs a pull request opened ready for review (not a draft) | upstream: GitHub "ready for review" | loose | "ready for review" in L904 |
| Lead Branch / lead branch | docs/commands.md:1078, docs/commands.md:1076 | `` `origin/<lead branch>` does not exist `` — the same branch written `lead branch` in a placeholder and Lead Branch in prose | internal: CONTEXT.md:243 | loose | placeholders `<lead branch>` keep the code's name `lead_branch`; prose Lead Branch |
| hold / holders | docs/commands.md:1371, docs/commands.md:177 | `` holds the PR's `open` Job `` — a review draft "holds" the open Job; resource "holders" | internal: Resource Tenant (CONTEXT.md:295) | loose | keep |
| §n convention (line 3) | docs/commands.md:3 | `` without a prefix means contract.md §n `` — statement that an unprefixed §n is contract.md | internal: the body uses unprefixed §n for commands.md itself (for example §9.1, §5.1, §3.15) | wrong | say "§n of this file; contract.md §n with the prefix" (the body does) |
| *Fine (one line)* | — | Plan, Context record, Context store, Grounding, Clarifying, Specifying, Dividing, Sequencing, Approving, Dispatching, Mechanical checks, Reviewing, Monitoring, Addressing, CPD (each once, L9 only), Job, Spec, Review Job, Dependency, Attempt, Locked Job, Session, Event, Inbox, Inbox item, Cockpit, Engine, Feedback item, Pending work, Lead, Worker, Manager, User, Escalation, Gate, Quick check, Lead Branch, Tier, Policy, Resource, Tenant, Skipped, Fallback, Attach | used as CONTEXT.md defines them, no collision seen beyond the rows above (read, not machine-checked) | — | ok | — |
| *Not in the part by that name* | — | Distilled Context, Job kind, Open Job, Update Job, Ledger, Draft, Brief, Merged, Axiom, Domain, Scope, Repo policy file, Exposure, Policy key, Slot, Machine policy, MCP server, Resolved policy, Known owner (`rg -w`, zero hits; the part writes "`open` Job", "`update` Job", "digest", "brief", "`merged`" in lower case) | capitalisation differs from CONTEXT.md; no collision | — | loose | none (the lower-case `open` Job names the kind in code font) |

## PR occurrences

Sense codes: `object` (the cpd object), `pull request`, `phase`, `step`, `mixed`, `other`. Every capitalised `PR`/`PRs` (305) was assigned by reading it in its sentence; the default is `object`, and the 21 that are not are listed one by one below. There is no `phase` and no `other` use of capitalised `PR` in the part.

### Totals

| Sense | `PR`/`PRs` | `pull request(s)` | Identifiers (below) |
|---|---|---|---|
| object | 284 | — | 100 |
| pull request | — | 28 | 23 |
| phase | 0 | — | 0 |
| step | 1 | — | 0 |
| mixed | 20 | — | 32 |
| other | 0 | — | 9 |
| **total** | 305 | 28 | 164 |

### By section (the `object` and `pull request` groups; the `mixed` and `step` ones are in the next table)

| Where | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| docs/commands.md:113 (§1.3 Selection and common refusals) | 40 | object | `` `pr_required` \| 2 \| the command needs a PR `` | PR -> Workflow; rule R1 |
| docs/commands.md:104 (§1.3 Selection and common refusals) | 9 | pull request | `` it polls every open pull request of the User `` | unchanged |
| docs/commands.md:456 (§3 `context` (3.1–3.18)) | 25 | object | `` no PR of a Plan of the Context that a PR of a Plan outside `` | PR -> Workflow |
| docs/commands.md:614 (§3 `context` (3.1–3.18)) | 4 | pull request | `` its PRs had no pull request or pull requests of two owners `` | unchanged |
| docs/commands.md:791 (§4.1–4.4 `plan new`, `list`, `show`, `phase`) | 6 | object | `` its PRs are added later with `pr add` `` | PR -> Workflow |
| docs/commands.md:837 (§4.1–4.4 `plan new`, `list`, `show`, `phase`) | 1 | pull request | `` `pull_request` (hs-d96: its row of contract §2.2 `` | unchanged |
| docs/commands.md:904 (§4.5 `plan render` and the Plan summary (with its sample)) | 65 | object | `` a table of the PRs that are not `merged` `` | PR -> Workflow (includes the rendered headings, next tables) |
| docs/commands.md:904 (§4.5 `plan render` and the Plan summary (with its sample)) | 2 | pull request | `` pushes and opens the pull request ready. `` | unchanged |
| docs/commands.md:1013 (§4.6–4.7 `plan review`, `plan decide`) | 9 | object | `` every PR of shape `tdd` that has not opened `` | PR -> Workflow |
| docs/commands.md:1047 (§4.6–4.7 `plan review`, `plan decide`) | 1 | pull request | `` reaches the pull request by its own merge `` | unchanged |
| docs/commands.md:1077 (§4.8 `plan lock`) | 40 | object | `` its Lead Branch and its parent path do not exist `` | PR -> Workflow |
| docs/commands.md:1269 (§4.9–4.14 `plan rename` … `plan delete`) | 24 | object | `` **no open PR**: every PR of the Plan is `merged` or `cancelled` `` | PR -> Workflow |
| docs/commands.md:1172 (§4.9–4.14 `plan rename` … `plan delete`) | 8 | pull request | `` closes the Plan's pull requests that are not merged `` | unchanged |
| docs/commands.md:1358 (§5 `job` (5.1–5.9)) | 68 | object | `` the PR's stored shape allows the kind `` | PR -> Workflow |
| docs/commands.md:1371 (§5 `job` (5.1–5.9)) | 1 | pull request | `` reaches its pull request by its own merge `` | unchanged |
| docs/commands.md:1598 (§6 `attempt` (6.1–6.4)) | 7 | object | `` the base PR's Lead Branch is on the remote `` | PR -> Workflow |
| docs/commands.md:1593 (§6 `attempt` (6.1–6.4)) | 2 | pull request | `` the newest pull request of the Lead Branch is not closed `` | unchanged |

### The `step` and every `mixed` use, one by one

| Where | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| docs/commands.md:9 | 1 | step | `` (Mechanical checks, PR, Reviewing and Monitoring, Addressing) `` | a step with another name (Fix list) |
| docs/commands.md:125 | 1 | mixed: `open` is the Workflow's status, `closed` the pull request's state | `` found the `open` PR's pull request not stored `closed` `` | Workflow's pull request |
| docs/commands.md:167 | 1 | mixed: the title of the row, used on the pull request | `` found a PR with no stored title `` | decide (Fix list) |
| docs/commands.md:168 | 1 | mixed: the Jira ticket of the row, for the pull request | `` found a PR whose policy sets `pr.jira_required` `` | decide |
| docs/commands.md:216 | 1 | mixed: as L167 | `` A PR's title (`--title` of `pr add` and `pr edit` `` | decide |
| docs/commands.md:216 | 1 | mixed: as L168 | `` A PR's Jira ticket (`--jira` of `pr add` `` | decide |
| docs/commands.md:542 | 1 | mixed: a Workflow is not opened, its pull request is | `` or null before the PR is opened `` | "before its pull request is opened" |
| docs/commands.md:773 | 1 | mixed: plan a Workflow that ends in a pull request | `` plan a docs PR for them `` | "a docs Workflow" |
| docs/commands.md:856 | 1 | mixed: as L168 | `` `jira` (the PR's Jira ticket, the key or null `` | decide |
| docs/commands.md:904 | 1 | mixed: rendered title of the placeholder Job | `` `Open the pull request: <PR title>` `` | `<pull request title>` |
| docs/commands.md:941 | 1 | mixed: rendered line of the Plan summary | `` base <base>): <PR title> `` | `<pull request title>` |
| docs/commands.md:941 | 2 | mixed: the definition of that placeholder and of the row title | `` `<PR title>` the PR's `title`, else `—` `` | pull request title |
| docs/commands.md:1002 | 1 | mixed: the rule of the row title | `` It has the rule of a PR's title `` | decide |
| docs/commands.md:1129 | 1 | mixed: the engine polls the pull request of the Workflow | `` the engine keeps polling its PR `` | "its pull request" |
| docs/commands.md:1340 | 1 | mixed: "open" is said of the pull request | `` because a PR with no change cannot open `` | "a Workflow with no change cannot open its pull request" |
| docs/commands.md:1371 | 1 | mixed: the Gate of the open/update path of the Workflow | `` no PR Gate runs `` | "no Gate runs" |
| docs/commands.md:1438 | 1 | mixed: `pr open` acts on a Workflow and opens its pull request | `` to open the PR later the Lead runs `pr open --pr <key>` `` | decide with the command name |
| docs/commands.md:1593 | 1 | mixed: as L167 | `` (kind `open`) the PR has a stored title `` | decide |
| docs/commands.md:1593 | 1 | mixed: as L168 | `` the PR has a stored `jira` when its policy sets `pr.jira_required` `` | decide |
| docs/commands.md:1472 | 1 | mixed: as L168 | `` the PR has no stored `jira` `` | decide |

### Identifiers: flags, JSON keys, error codes, Event kinds, tables, commands

Counts are of every match of the regex in the part. "Where" lists the lines (up to 14).

| Identifier | Where | Count | Sense | Quote (found at the first listed line that has it) | After the rename |
|---|---|---|---|---|---|
| `--pr <key>` (flag) | 104, 113, 1327, 1328, 1336, 1384, 1389, 1393, 1400, 1403, 1408, 1410, 1438 | 16 | object | `` `--pr <key>`, within the selected Plan `` | `--workflow <key>` |
| `pr_required` | 104, 113 | 3 | object | `` `pr_required` (exit 2) `` | `workflow_required` |
| `pr_not_found` | 104, 145, 1400 | 3 | object | `` is `pr_not_found` (exit 4) `` | `workflow_not_found` |
| `pr_ended` | 1346, 1401 | 2 | object | `` the PR is not `merged` or `cancelled` (§3.2) `` | `workflow_ended` |
| `pr_empty` | 1074 | 1 | object | `` `pr_empty` `` | `workflow_empty` |
| `pr_other_plan` | 131 | 1 | object | `` `pr_other_plan` `` | `workflow_other_plan` |
| `needs_other_pr` | 1349, 1403 | 2 | object | `` `needs_other_pr` `` | `needs_other_workflow` |
| `invalid_pr_id` | 135 | 1 | object | `` `invalid_pr_id` `` | `invalid_workflow_id` |
| `pr_adopted` | 175 | 1 | object | `` `pr edit --repo` or `--base` on an adopted PR `` | `workflow_adopted` |
| `pr_not_open` | 123 | 1 | object | `` found the PR not `open` `` | `workflow_not_open` |
| `pr_title_required` | 167, 1593 | 2 | mixed | `` found a PR with no stored title `` | `workflow_title_required`? or keep: the title is the pull request's (User decides) |
| `pr_not_closed` | 125 | 1 | mixed | `` not stored `closed` `` | split: Workflow `open`, pull request not closed; name e.g. `pull_request_not_closed` |
| `pr_not_ready` | 130 | 1 | pull request | `` `postable::unready(&row)` is not empty `` | keep (pull request) |
| `pr_closed` | 1593 | 1 | pull request | `` not closed without a merge `` | keep (pull request) |
| `pr_posts` (table) | 126 | 1 | mixed | `` found a `pr_posts` row for the PR `` | keep `pr_posts` if the post is the pull request's; Workflow key column renamed |
| `base_pr` (JSON key, `details.base_pr`) | 121, 132, 856, 861, 864, 945, 1598 | 7 | object | `` `base_pr` `` | `base_workflow` |
| `base_pr_unknown`, `base_pr_unavailable`, `base_pr_other_repo` | 131 | 3 | object | `` `base_pr_unknown` `` | `base_workflow_*` |
| `prs` (key or table) | 542, 547, 614, 626, 811, 824, 842, 856, 861, 925, 1015, 1074, 1083, 1087 … | 16 | object | `` `prs` `` | `workflows` |
| `prs.repo` | 330, 908 | 2 | object | `` the same `prs.repo` realpath as stored `` | `workflows.repo` |
| Event `pr-started` | 1083 | 1 | object | `` one `pr-started` `` | `workflow-started` |
| Event `pr-opened` | 1608 | 1 | mixed | `` `pr-opened` and `plan-phase` follow `` | `pull-request-opened`? (the Workflow moves to `open`; the pull request opens) |
| Event `pr-feedback` | 1188 | 1 | pull request | `` no `pr-feedback` for an archived Plan `` | keep |
| Event `pr-base-changed` | 71 | 1 | object | `` 5 until `pr-base-changed`, hs-d324 `` | `workflow-base-changed` (history text: keep the old word in the protocol list? User decides) |
| migration `pr-title` | 167 | 1 | other | `` existed before migration `pr-title` `` | keep (applied migration = history) |
| `pr add` | 104, 121, 131, 169, 170, 171, 172, 173, 174, 180, 216, 256, 791, 1074 … | 19 | object | `` `pr add` (§9.4) `` | `workflow add` |
| `pr edit` | 104, 121, 131, 167, 168, 175, 216, 1472 | 9 | object | `` `pr edit` (§9.4, §9.5) `` | `workflow edit` |
| `pr cancel` | 104, 125, 185, 1340, 1427, 1438 | 6 | object | `` `pr cancel` (§9.6) `` | `workflow cancel` |
| `pr start` | 104, 1081 | 2 | object | `` `pr start` `` | `workflow start` |
| `pr open` | 104, 121, 122, 167, 168, 189, 256, 837, 1082, 1438 | 10 | mixed | `` `pr open` (§9.1) `` | `pr open` acts on a Workflow and opens its pull request: User decides (`workflow open`?) |
| `pr push` | 104, 120, 256, 1371 | 4 | mixed | `` `pr push` found the remote branch moved `` | as `pr open` |
| `pr rebase` | 104, 132, 133, 256, 1371 | 6 | mixed | `` `pr rebase` (§9.3) found the PR stacked `` | as `pr open` |
| `pr poll` | 104, 192, 540, 542, 1188 | 5 | pull request | `` `pr poll` has no `--pr` `` | keep |
| `pr post` | 104, 126, 127, 128, 129, 130, 156, 161, 248 | 9 | pull request | `` `pr post` (§9.8b) found `` | keep |
| `pr reply` | 104, 123, 124, 247 | 4 | pull request | `` `pr reply` (§9.8a) found the PR not `open` `` | keep (acts on the pull request's feedback; takes `--pr`: mixed at flag level) |
| `pr merge` | 146 | 1 | pull request | `` `pr merge` (§9.10) re-read the pull request `` | keep |
| `pr view` | 104 | 1 | pull request | `` `pr view` `` | keep (takes `--pr`) |
| `gh pr close`, `gh pr list` (GitHub CLI) | 1179, 1188, 1606 | 3 | other | `` `gh pr close <number> --repo `` | keep (upstream gh) |
| `pr_open_job` (test names in Spec samples) | 1314, 1315, 1562 | 5 | other | `` "cli/tests/it/pr_open_job.rs" `` | follow the code if the test file is renamed; judgement |
| `pr-<key>.pre-merge.log` (log file) | 1600, 1606 | 2 | object | `` `run/<plan>/logs/pr-<key>.pre-merge.log` `` | `workflow-<key>.pre-merge.log` (file name in code) |
| `open-<pr>` (placeholder Job key) | 904 | 1 | object | `` key `open-<pr>` `` | `open-<workflow>` |
| policy keys `gate.pr`, `pr.jira_required`, `pr.post.*`, `policy.pr.post.enabled` | 127, 129, 168, 1472, 1593, 1600 | 7 | mixed | `` `pr.jira_required` `` | Domain `pr` holds keys of both kinds; User decides the split |

`pr_in_use` and `pr_locked` do not occur. Not an identifier of the object: `owner_in_use` (L626) carries the key `prs`.

### JSON keys `pr` and `kind: pr`

| Where | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| docs/commands.md:837 (`prs[].pr` of `plan list`) | 1 | mixed | `` `pr` (`{number, url, opened_at}` or null) `` | rename `prs[]`; this `pr` is the pull request while `jobs[].pr` is the Workflow key |
| docs/commands.md:856 (`plan show` field list) | 1 | mixed | `` `lead_worktree`, `pr`, `pull_request` `` | same |
| docs/commands.md:862 (`plan show` sample) | 1 | mixed | `` "pr": null, "pull_request": null `` | same |
| docs/commands.md:926 (`overlaps[]` sample) | 1 | object | `` "pr": "api", "other_jobs" `` | `"workflow": "api"` |
| docs/commands.md:1195 (`plan cancel` sample) | 1 | mixed | `` "closed_prs": [{"pr": "ui", "number": 813}] `` | Workflow key + pull request number; rename `closed_prs` |
| docs/commands.md:1188 (`plan cancel` step 5) | 1 | mixed | `` listed under `left_open` (`[{pr, number}]`) `` | as above |
| docs/commands.md:1371 (`job add` Effects) | 1 | object | `` `pr` = the PR's key `` | `workflow` |
| docs/commands.md:1376 (`job add` sample) | 1 | object | `` "job": {"key": "api-schema", "pr": "api" `` | `workflow` |
| docs/commands.md:1498 (`job list` sample) | 1 | object | `` {"key": "api-schema", "pr": "api" `` | `workflow` |
| docs/commands.md:1510 (`job show` sample) | 1 | object | `` "job": {"key": "worker", "pr": "api" `` | `workflow` |
| docs/commands.md:1129 (`plan archive`) | 1 | object | `` (`kind` `pr`, `id` `<plan>/<key>` `` | `kind` `workflow` |

### Rendered headings and labels

| Where | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| docs/commands.md:556 (`context pending --markdown` heading) | 1 | object | `` ### PR <key> (<repo>, base <base>): <status> `` | `### Workflow <key> …` |
| docs/commands.md:904 (`plan render`) | 1 | object | `` headed `PR <key>`, with its repo, base and needs `` | `Workflow <key>` |
| docs/commands.md:941 (`plan render` summary layout) | 1 | object | `` `## PRs`, then for each PR that counts `` | `## Workflows` |
| docs/commands.md:961 (`plan render` sample) | 1 | object | `` ## PRs `` | `## Workflows` |
| docs/commands.md:917 (`plan render` Overlaps) | 1 | object | `` \| Job \| Other Plan \| PR \| Its Jobs (status) \| Paths \| `` | `Workflow` |
| docs/commands.md:948 (`plan render` summary block 10) | 1 | object | `` then `PR <key>` for each PR that counts `` | `Workflow <key>` |
| docs/commands.md:945 (summary block 7) | 1 | object | `` one `<key> on <base PR>` per PR that counts `` | `<key> on <base Workflow>` |
| docs/commands.md:904 (placeholder Job title) | 1 | mixed | `` `Open the pull request: <PR title>` `` | `<pull request title>`? the title is the pull request's; User decides |
| docs/commands.md:941 (summary block 3) | 1 | mixed | `` base <base>): <PR title> `` | as above |

(These rendered headings are already counted among the capitalised `PR` of the sections above, except the two `<PR title>` ones, which are in the `mixed` table. The `## PRs` on L961 is inside the sample of L941.)

## Fix list

Only what needs a judgement is listed one by one. The User decides every contested wording; each row proposes.

| path:line | What to change | Why |
|---|---|---|
| docs/commands.md:3 | `` without a prefix means contract.md §n `` — say that an unprefixed §n is commands.md and a contract reference carries "contract.md" | the body writes §9.1, §5.1, §3.15 for this file and "contract §7.6" for the contract: the sentence is the reverse of the practice (wrong) |
| docs/commands.md:9 | `` (Mechanical checks, PR, Reviewing and Monitoring, Addressing) `` — rename the step "PR" (step code `step`) | "PR" must mean only the pull request; the step is the opening of it; CONTEXT.md:71 changes with it |
| docs/commands.md:167 | `` found a PR with no stored title `` — decide: Workflow title or pull request title (`mixed`) | the code stores `title` on the row and puts it on the pull request; the error code `pr_title_required` follows the decision |
| docs/commands.md:168 | `` found a PR whose policy sets `pr.jira_required` `` — same decision for the Jira ticket (`mixed`) | policy key `pr.jira_required` and column `jira` of the row |
| docs/commands.md:216 | `` A PR's title (`--title` of `pr add` and `pr edit` `` — `mixed`: "A Workflow's title" or "A pull request's title" | one title serves both; the flag `--title` of `pr add`/`pr edit` is on the object |
| docs/commands.md:216 | `` A PR's Jira ticket (`--jira` of `pr add` `` — `mixed`, as above | Jira is on the row and on the pull request; CONTEXT.md:9-11 puts the Jira ticket at the Plan level |
| docs/commands.md:542 | `` or null before the PR is opened `` — `mixed`: "before its pull request is opened" | a Workflow is not opened; its pull request is |
| docs/commands.md:773 | `` plan a docs PR for them `` — `mixed`: "plan a docs Workflow for them" | the Manager plans a Workflow (a PR of a Plan) that ends in a pull request |
| docs/commands.md:856 | `` `jira` (the PR's Jira ticket, the key or null `` — `mixed`, as L168 | same field |
| docs/commands.md:904 | `` `Open the pull request: <PR title>` `` — `mixed`: `<pull request title>` | rendered title of the placeholder Job |
| docs/commands.md:941 | `` base <base>): <PR title> `` — `mixed`: `<pull request title>` (three occurrences in the line) | rendered line of the Plan summary |
| docs/commands.md:1002 | `` It has the rule of a PR's title `` — `mixed`: "the rule of a Workflow's title" | `--title` of `plan review` copies the rule of the row title |
| docs/commands.md:1129 | `` the engine keeps polling its PR `` — `mixed`: "polling its pull request" | the engine polls the pull request, not the Workflow |
| docs/commands.md:1340 | `` because a PR with no change cannot open `` — `mixed`: "a Workflow with no change cannot open its pull request" | "open" is said of the pull request |
| docs/commands.md:1371 | `` no PR Gate runs `` — `mixed`: "no Gate runs" | the Gate runs on the pull request path of a Workflow; "PR Gate" is level wording |
| docs/commands.md:1438 | `` to open the PR later the Lead runs `pr open --pr <key>` `` — `mixed`: "to open its pull request later … `pr open`" | command acts on a Workflow and opens its pull request |
| docs/commands.md:1593 | `` (kind `open`) the PR has a stored title `` — `mixed`: Workflow or pull request title (as L167) | same field |
| docs/commands.md:1593 | `` the PR has a stored `jira` when its policy sets `pr.jira_required` `` — `mixed`, as L168 | same field |
| docs/commands.md:1472 | `` the PR has no stored `jira` `` — `mixed`, as L168 | same field |
| docs/commands.md:125 | `` found the `open` PR's pull request not stored `closed` `` — `mixed`: "found the `open` Workflow's pull request" | the status `open` is the Workflow's; `closed` is the pull request's state |
| docs/commands.md:1076 | `` its workflow worktree path does not exist `` — replace "workflow worktree" by "Lead worktree" (L1076, L1079, L1580, L1600) | cpd 1 word for the PR's worktree; after the rename it would pass unnoticed as the new Workflow |
| docs/commands.md:115 | `` `context retire` or `context docs` (§3.16, §3.17) named a record `` — §3.16, §3.17 -> §3.17, §3.18 | `context retire` is §3.17 and `context docs` §3.18 (wrong reference) |
| docs/commands.md:116 | `` (§3.15 to §3.17) ran with `` — §3.15 to §3.17 -> §3.16 to §3.18 | `context curation` is §3.16 (wrong reference) |
| docs/commands.md:117 | `` `context docs --send` (§3.17) found no live Manager `` — §3.17 -> §3.18 | `context docs` is §3.18 (wrong reference) |
| docs/commands.md:118 | `` `context docs` (§3.16, §3.17) named one id `` — §3.16, §3.17 -> §3.17, §3.18 | wrong reference |
| docs/commands.md:233 | `` `cpd context owner` (§3.13) `` — §3.13 -> §3.14 | `context owner` is §3.14 (wrong reference) |
| docs/commands.md:244 | `` `cpd context curation` (§3.15, internal) `` — §3.15 -> §3.16 (L244), §3.16 -> §3.17 (L245), §3.17 -> §3.18 (L246) | sections 3.15 (`context delete`) was inserted before them (wrong references) |
| docs/commands.md:585 | `` Inbox items `curate-retire` and `curate-docs`, §3.15–§3.17 `` — §3.15–§3.17 -> §3.16–§3.18 | wrong reference |
| docs/commands.md:659 | `` the Context's Manager (§3.10) is not in the chat slot `` — §3.10 -> §3.11 | `context manager` is §3.11 (wrong reference) |
| docs/commands.md:736 | `` are not set in the environment (as §3.15) `` — §3.15 -> §3.16 (L736, L767) | `context curation` states the rule, §3.16 (wrong reference) |
| docs/commands.md:801 | `` `plan new` and `context manager` use it (§3.10) `` — §3.10 -> §3.11 | wrong reference |
| docs/commands.md:1685 | `` `context ask` / `context answer` (§3.4, §3.5) `` — §3.4, §3.5 -> §3.5, §3.6 | `context search` is §3.4 (wrong reference) |
| docs/commands.md:656 | `` **archived or asleep**: the Context is archived `` — define or replace "asleep" (also L1267) | a word with no glossary entry; the code `archive_blocked` is also used for `context delete`/`plan delete` refusals (L656-L658, L1267-L1269) |
| docs/commands.md:837 | `` `pr` (`{number, url, opened_at}` or null) `` — resolve the double meaning of the JSON key `pr` (L837, L856, L862, L865, L867) | `prs[].pr` is a pull request; `jobs[].pr` and `details.pr` are Workflow keys (overloaded) |
| docs/commands.md:542 | `` or `waiting` (hs-d523: display only `` — rename the Job display value `waiting` (L542, L904) or say it is not a status | a Job has no `waiting` status; Workflow status `waiting` and Session state `waiting` exist (overloaded) |
| docs/commands.md:542 | `` display only, blocked by a dependency or PR that has not started `` — "waiting on a Dependency or Workflow that has not started" | Axiom "Blocked means it needs the human"; Session `blocked` means that (loose) |
| docs/commands.md:904 | `` the Jobs that are not `draft` are Locked context `` — "are Locked Jobs, shown as context" | "Locked context" reads as the Context entity (loose) |
| docs/commands.md:582 | `` skip non-curan Contexts `` — "non-curable Contexts" | typo (loose) |
| docs/commands.md:556 | `` (§hs-d521) `` — "(hs-d521)" | typo: "§" before a record id (loose) |
| docs/commands.md:1675 | `` ## 7. `review` `` — keep the tombstone, or delete §7 and §7.1 if no file cites them | section kept for a removed command group (stale; check who cites §7 before deleting) |
| docs/commands.md:1600 | `` `gate.pr` is `local` `` — pick one spelling of the Gate key across CONTEXT.md (`pr.gate`) and the contract/commands (`gate.pr`) | CONTEXT.md and this file disagree; contract.md:3561 uses `gate.pr` (unverified which one the code reads) |
| docs/commands.md:798 | `` Jira task key, stored only `` — "Jira ticket key" for a Plan; decide the PR/Workflow Jira field (L216, L856) | CONTEXT.md:9-11 maps Ticket = Plan and Task = Job; Jira "task" is an issue type (overloaded) |
| docs/commands.md:5 | `` every command of outline §3 `` — "the design outline" (L5, L69), with its path | "outline" also names the Plan outline of §4.13 (overloaded) |
| docs/commands.md:1314 | `` "cli/tests/it/pr_open_job.rs" `` — follow the file name if the code renames it (L1314, L1315 three times, L1562) | test name in Spec samples; not a statement about cpd (judgement) |
| docs/commands.md:1195 | `` "closed_prs": [{"pr": "ui", "number": 813}] `` — rename `closed_prs` and `left_open` `[{pr, number}]` (L1172, L1188, L1195) | Workflow key plus pull request number (`mixed`) |
| docs/commands.md:71 | `` 5 until `pr-base-changed`, hs-d324 `` — decide: the protocol history lists the Event kind `pr-base-changed`; keep as history or rename with the Event | Event kinds are identifiers (renamed) but the sentence is a dated history |
| docs/commands.md:167 | `` existed before migration `pr-title` `` — keep | applied migration name: history keeps the old words |

### Rules (mechanical, no row each)

| Rule | Applies to | Count |
|---|---|---|
| R1: every `object` `PR`/`PRs` becomes Workflow/Workflows (articles and "base PR" -> "base Workflow", "PR key" -> "Workflow key") | §1.3 Selection and common refusals, §3 `context` (3.1–3.18), §4.1–4.4 `plan new`, `list`, `show`, `phase`, §4.5 `plan render` and the Plan summary (with its sample), §4.6–4.7 `plan review`, `plan decide`, §4.8 `plan lock`, §4.9–4.14 `plan rename` … `plan delete`, §5 `job` (5.1–5.9), §6 `attempt` (6.1–6.4) | 284 |
| R1 in §1.3 Selection and common refusals | `object` | 40 |
| R1 in §3 `context` (3.1–3.18) | `object` | 25 |
| R1 in §4.1–4.4 `plan new`, `list`, `show`, `phase` | `object` | 6 |
| R1 in §4.5 `plan render` and the Plan summary (with its sample) | `object` | 65 |
| R1 in §4.6–4.7 `plan review`, `plan decide` | `object` | 9 |
| R1 in §4.8 `plan lock` | `object` | 40 |
| R1 in §4.9–4.14 `plan rename` … `plan delete` | `object` | 24 |
| R1 in §5 `job` (5.1–5.9) | `object` | 68 |
| R1 in §6 `attempt` (6.1–6.4) | `object` | 7 |
| R2: identifiers of the object take the new noun (flag `--pr` -> `--workflow`, codes `pr_*`, `needs_other_pr`, `base_pr*`, table and key `prs`, Event `pr-started`, commands `pr add`, `pr edit`, `pr cancel`, `pr start`, log file `pr-<key>.pre-merge.log`, placeholder `open-<pr>`) | the `object` identifier rows | 100 |
| R3: every `pull request` identifier and every capitalised `PR` that means the pull request keeps its word (`pr poll`, `pr post`, `pr reply`, `pr merge`, `pr view`, `pr-feedback`, `pr_not_ready`, `pr_closed`) | `pull request` identifier rows; `pull request(s)` in prose | 23 + 28 |
| R4: history keeps the old words (migration `pr-title`, "cpd 1 §11 `workflow …`", test and migration names, applied history) | `other` rows and L783, L876, L884 | 9 + 3 |
| R5: the `mixed` rows are decided one by one (table above), the commands `pr open`, `pr push`, `pr rebase`, the Event `pr-opened` and the policy keys of Domain `pr` included | `mixed` | 20 + 32 |

## Counts

| What | Number |
|---|---|
| files audited | 1 (`docs/commands.md`) |
| files read for the glossary | 1 (`CONTEXT.md`, 323 lines) |
| lines audited, read in full | 1730 |
| rows in `## Terms` | 37 + 2 summary lines |
| rows in `## PR occurrences` | 17 (by section) + 20 (step and mixed) + 41 (identifiers) + 11 (JSON keys) + 9 (rendered headings) |
| rows in `## Fix list` | 47 + 14 rules |
| capitalised `PR`/`PRs` | 305 |
|   `object` | 284 |
|   `pull request` | 0 |
|   `phase` | 0 |
|   `step` | 1 |
|   `mixed` | 20 |
|   `other` | 0 |
| `pull request(s)`, any case | 28 |
|   `pull request` sense | 28 |
| whole-word `workflow(s)` | 7 (L783, L876, L884: history; L1076, L1079, L1580, L1600: stale) |
| identifier matches by sense code (`object` / `pull request` / `mixed` / `other`) | 100 / 23 / 32 / 9 |

