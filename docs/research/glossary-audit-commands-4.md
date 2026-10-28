# Glossary audit: commands.md sections 15 to 20 (adapter, Worker, TUI reads)

Audit of `docs/commands.md` lines 3648 to 4512, Job `aud-commands-4`, Attempt `aud-commands-4-a1`. Report only: no file other than this one was changed. Terminology as of CONTEXT.md and the decisions pro-d136 and pro-d138 (the cpd object `PR` becomes Workflow).

## Coverage

| File | Lines | Heading range | Read |
|---|---|---|---|
| `docs/commands.md` | 3648-3651 | §15 intro | in full |
| `docs/commands.md` | 3652-3661 | §15.1 | in full |
| `docs/commands.md` | 3662-3680 | §15.2 | in full |
| `docs/commands.md` | 3681-3716 | §15.3 | in full |
| `docs/commands.md` | 3717-3732 | §15.4 | in full |
| `docs/commands.md` | 3733-3791 | §15.5 | in full |
| `docs/commands.md` | 3792-3836 | §15.6 | in full |
| `docs/commands.md` | 3837-3897 | §15.7, §15.7a | in full |
| `docs/commands.md` | 3898-3924 | §15.8 | in full |
| `docs/commands.md` | 3925-3981 | §15.9, §15.10 | in full |
| `docs/commands.md` | 3982-4046 | §15.11, §15.12 | in full |
| `docs/commands.md` | 4047-4108 | §15.13 to §15.15 | in full |
| `docs/commands.md` | 4109-4147 | §15.16 | in full |
| `docs/commands.md` | 4148-4221 | §15.17 to §15.19 | in full |
| `docs/commands.md` | 4222-4241 | §15.20 | in full |
| `docs/commands.md` | 4242-4292 | §16 | in full |
| `docs/commands.md` | 4293-4417 | §17 | in full |
| `docs/commands.md` | 4418-4438 | §18 | in full |
| `docs/commands.md` | 4439-4483 | §19 | in full |
| `docs/commands.md` | 4484-4512 | §20 | in full |

The part is `## 15. The pi adapter's protocol` (line 3648) to the line before `## 21. \`inbox\`` (line 4513): lines 3648 to 4512, 865 lines, all in one file. Read in full: all 865 lines, in chunks of 215, 260, 200 and 190 lines (the tool limit forced larger chunks than the Spec's 150). Skimmed: none. Not read: none. Outside the part, read only to check a claim: `docs/commands.md:837` and `:856` (the `plan show` fields `pr` and `pull_request`), not audited.

Commands behind each count (run from the worktree; `<PART>` is `sed -n 3648,4512p docs/commands.md`):

- Section headings: `rg -n '^## 1[5-9]\.|^## 2[0-1]\.' docs/commands.md` (lines 3648, 4242, 4293, 4418, 4439, 4484, 4513) and `<PART> | rg -n '^#{2,4} '`.
- PR word hits (the unit of `## PR occurrences`, 126): `<PART> | rg -o -i '\bprs?\b|pull[ _]requests?' | wc -l` gives 126; of these `<PART> | rg -o -i 'pull[ _]requests?' | wc -l` gives 18 and `<PART> | rg -o -i '\bprs?\b' | wc -l` gives 108 (60 are the capitalised `PR`/`PRs`: `<PART> | rg -o -w 'PRs?' | wc -l`).
- Per-line distribution of those hits: `<PART> | rg -n -o -i '\bprs?\b|pull[ _]requests?' | awk -F: '{print $1+3647}' | sort -n | uniq -c`; the table below is checked against it line by line (sum of Count per line equals the hits of that line).
- Identifiers, counted apart: `<PART> | rg -o '\bpr_[a-z_]+|\bPR_[A-Z_]+' | wc -l` gives 12 hits on 11 distinct identifiers (`pr_look` appears twice at 4381: in `pr_look(` and in the file name `pr_look.rs`).
- Plan phase check: `<PART> | rg -n phase` (lines 3746, 3774, 3858, 3886, 4049, 4309, 4383, 4385, 4390, 4425, 4459): none names a `pr` phase; the `pr` phase is gone from the part.
- Workflow: `<PART> | rg -c -i workflow` gives 7 (4251, 4295, 4416, 4423, 4424, 4425, 4434), all cpd 1 history except 4251.
- Word-to-watch lines: `<PART> | rg -n -o -i '.{0,45}(<word>).{0,45}'` per word (tier, ready, brief, Ledger, Dispatch column, Deploy column, `deploy`, job-merged, Reviewer, stage, owner, board post, resource, queue, note, lock).
- Terms absent from the part (searched, none found): Plan stage, Clarifying, Specifying, Dividing, Sequencing, Approving, Mechanical checks, Monitoring, Addressing, task, ticket (none; `jira_required` at 4273 is an identifier), epic, and the word `stage`.

## Terms

| Term | Where (path:line) | Sense as used here | Collision (internal: another cpd meaning; or upstream: GitHub, Jira, git, tmux, pi) | Verdict | Proposed wording |
|---|---|---|---|---|---|
| Workflow | docs/commands.md:4251 | cpd 1's name for a Plan (retired by D3): "(was "Workflow")" | internal: returns as the new name of the cpd object that is now `PR` | stale | drop the parenthesis, or write "(cpd 1 called a Plan a workflow)"; after the rename a reader takes "Workflow" for the new object |
| Workflow (cpd 1 column) | docs/commands.md:4434 | cpd 1 row of the history table: one watch per Plan: "`watch --wf` per Workflow" | internal: same as above | ok | history: keep; optional one-line note under the §18 table that cpd 1's workflow means Plan |
| Workflow (cpd 1 command) | docs/commands.md:4424 | cpd 1 command, answered by `plan new`; the same row says each repo "is a PR, added with `pr add`": "`workflow init`" | internal: cpd 1 `workflow init` vs the new Workflow that `pr add` makes | loose | history: keep the cpd 1 cell; the cpd 2 cell becomes "each repo of the Plan is a Workflow, added with `workflow add`"; a reader will see two Workflows in one row |
| Dispatch column | docs/commands.md:4341 | the right-hand cockpit column of host load and Inbox: "The **Dispatch column** follows `session-state` Events" | internal: Dispatch (the Plan stage); the same column is called "Deploy column" at 4311, 4301, 4319, 4365 | stale | "Deploy column" (the name the section's own table uses); also the "Dispatch row" at 4381 and 4385 |
| Deploy (column) | docs/commands.md:4311 | a cockpit column showing CPU, GPU, memory and the Inbox: "`widgets` (Deploy column)" | internal: Deploy stage/phase (4385 "deploy yellow 11"); Inbox kind `deploy` (4398) | overloaded | the column has nothing to do with the Deploy stage; User decides a different name (guess: "Host column"); until then write "Deploy column" in full, never bare "Deploy" |
| `deploy` (Inbox kind) | docs/commands.md:4398 | an Inbox item kind, urgency yellow: "`deploy` links no Session" | internal: Deploy stage, Plan phase `deploy`, Deploy column | overloaded | keep (it is an identifier); write "the `deploy` Inbox item" in prose |
| merged / `job-merged` | docs/commands.md:3687 | Event of a Job whose Attempt merged; for an `open` Job it "is the Lead's word that the pull request is open": "The `job-merged` of an `open` Job" | internal: Job and Attempt outcome `merged`, Workflow status `merged`, pull request merged, merge marker (4301) | wrong | the Event name says merged, the sentence says opened; propose: "the `job-merged` Event of an `open` Job reports that its pull request is open", and let the User decide whether it deserves its own kind |
| open | docs/commands.md:3687 | Job kind `open` (Open Job): "The `job-merged` of an `open` Job" | internal: pull request state open (4381), Workflow status `open`, open Escalation (4310), `pr open` (3923), `session open` (4330), Inbox item open (3698) | overloaded | write "Open Job" (CONTEXT entry) for the Job; "open pull request" for GitHub's state; keep the others as identifiers |
| draft / Draft | docs/commands.md:4049 | Draft Job (cpd): "a Plan with draft Jobs" | upstream: GitHub draft pull request, used in the same part (4381 "open and a draft") | overloaded | write "draft pull request" in full; "Draft Job" for cpd's |
| review | docs/commands.md:4433 | GitHub's review of the pull request: "the review is the PR's own, on GitHub" | internal: Plan review (3724 `plan-review:<review>`), Review Job (4268), removed Reviewer (4433); upstream: GitHub review | overloaded | "Plan review", "Review Job" and "the pull request's review" in full; bare "the review" only where the line already names one |
| Reviewer | docs/commands.md:4433 | cpd 1 role, removed by R19: "a Reviewer per Attempt" | internal: Review Job | ok | history: keep |
| Tier | docs/commands.md:4405 | urgency band of a dot or an Inbox item (red 0 .. shiro 4); also "blue tier" (4354) and "tier glyph" (4357): "Rows are ordered first by dot-urgency tier" | internal: Tier = model strength of a Session (3703 "its Session's Tier changed"; 4251) | overloaded | "urgency band" for the dots and the Inbox; keep Tier for the model. User decides |
| session / Session | docs/commands.md:3668 | pi's session (name, file, `session_start`) and cpd's Session in one sentence: "if pi's session name differs from the Session's `name`" | internal: Session; upstream: pi session, session file, `session_start` | loose | write "pi session" for pi's and "Session" for cpd's; most of the part already does (3650 "every pi session cpd starts") |
| run | docs/commands.md:3729 | one pi agent run: "the current run is a **reply run**" | internal: `session run`, `cpd resource run` (4236), Attempt run, test run (4021); upstream: `gh run watch` (4001), GitHub Actions run | overloaded | "agent run" for pi's; keep identifiers; a GitHub Actions workflow or run is written in full |
| check / checks | docs/commands.md:4252 | Quick check (`pre-commit`): "run the quick check yourself" | internal: Gate, "checked statement" (3923), protocol check (3663); upstream: GitHub CI checks (4383 "pull request checks") | overloaded | "Quick check", "Gate", "protocol check" and "CI checks" in full; "checked statement" (3923) is the reminder rule, not a check of the Gate: propose "stated" |
| block / blocking | docs/commands.md:3982 | a Manager or Lead never waits in its own turn: "A Manager or a Lead never blocks" | internal: Session state `blocked` (needs the human), "blocking reason" (3694) of kind agent that gives `waiting` not `blocked` (3727), Blocked axiom | loose | "never waits in its turn" for 15.11; 3694's agent-kind "blocking reason" contradicts "blocked means the human": propose "state reason" (reason of kind human or agent); User decides |
| blocking reason (agent kind) | docs/commands.md:3694 | a reason of kind agent for the Session state: "opens the blocking reason `escalation:<id>`" | internal: Session state `blocked` is for the human only (3727: agent reasons give `waiting`) | wrong | a reason that never makes a Session blocked is not a "blocking" reason; propose "state reason" (identifier prefixes `escalation:`, `plan-review:` stay) |
| context / Context | docs/commands.md:3837 | Context entity (records, board posts): "Context updates at turn start" | upstream: pi `ctx`, `ctx.ui` (3738), the model's `contextWindow` (4206); internal: `cpd-context` message | overloaded | "Context update" in capitals is already the term; write "context window" in full for the model's and "pi context" for `ctx` |
| ready | docs/commands.md:3693 | Workflow or pull request ready (not stated): "the PR stays ready" | internal: Ready Job (4312), `pr-ready` Inbox kind (4398), "ready to merge" (4381) | overloaded | three senses of ready in one part; propose "merge-ready" for the pull request and keep "Ready Job" |
| base | docs/commands.md:3693 | field `base` of the cpd object, which follows the pull request's base on GitHub: "the PR's `base` followed GitHub" | upstream: GitHub base branch; internal: another Workflow as base ("its base PR") | overloaded | after the rename: "the Workflow's `base`" and "the pull request's base branch" |
| status / state | docs/commands.md:4381 | pull request `state` words draft, open, closed, merged: "when the PR is open and a draft" | internal: Workflow `status` (`waiting`, `dispatch`, `open`, `merged`, `cancelled`, 4310), Job status, Session state (`waiting`, `blocked`) | overloaded | status words `open` and `merged` exist on both the Workflow and the pull request; write "Workflow status" and "pull request state" in full |
| decision | docs/commands.md:3841 | Context record of kind decision: "new decision records in force by other Sessions" | internal: Plan review decision (`cpd-plan-decision` 3774, `plan decide`), "Decision (User, …)" headings throughout | overloaded | "decision record" and "Plan review decision" in full |
| post | docs/commands.md:3841 | board post (Context): "the pending board posts to it" | internal: `pr post` (4414, a pull request comment), the menu entry **Post** | overloaded | "board post" vs "PR post" (CONTEXT entry): keep both, never bare "post" |
| poll / polling | docs/commands.md:3796 | adapter poll of its own directory: "An `unref`'d timer polls it every 5 seconds" | internal: `pr poll` (4464), events follow poll (4341 `events.poll_ms`) | loose | "directory poll", "PR poll" (CONTEXT: pull request poll), "Event poll" |
| lock | docs/commands.md:4301 | the engine's lock file (§1.4): "the engine lock probe" | internal: `plan lock` (3774, Locked Plan); "Reads take no lock" (4295) is SQLite's | overloaded | "engine lock" and "Plan lock" in full |
| brief | docs/commands.md:4202 | adjective: short: "(brief, from the spec)" | internal: Brief (a Manager's start message); removed "review brief" (4281) | loose | "short"; 4281's heading is removed-history, keep |
| Spec / spec | docs/commands.md:4202 | lower-case "spec" for a Job's Spec or the contract: "(brief, from the spec)" | internal: Spec (the Job's Spec, 4244) | loose | "the Spec" if a Job Spec is meant; the contract is "commands.md" or "contract.md" |
| owner | docs/commands.md:4087 | the Plan that owns a decision: "owner Plan `pi-theme`" | internal: Policy Known owner (4273 `owner_unset`), `<owner>/<repo>` (4414) | loose | "originating Plan" |
| branch | docs/commands.md:4238 | a Plan has no branch: the Lead Branch belongs to the cpd object (the PR): "for a Plan's branch" | internal: Lead Branch | wrong | "for a Workflow's Lead Branch" (after the rename) |
| repo | docs/commands.md:4113 | a Plan has one repo per cpd-object (R34), not one: "the directory of a Plan's repo, `prs.repo`" | internal: Workflow `repo` | wrong | "the directory of a Workflow's repo, `workflows.repo`" |
| policy | docs/commands.md:4252 | the repo's policy file of the cpd object's repo: "from the PR's policy" | internal: Repo policy file, Policy, Resolved policy | loose | "the repo policy file of the Workflow's repo" |
| Plan phase `plan` | docs/commands.md:3774 | phase name `plan` (grounding, plan, dispatch, deploy, done): "in phase `plan`" | internal: Plan (the entity); `PR` phase is gone: no `pr` phase appears in the part | ok | fine; phase `deploy` already replaces the old PR phase (4385 "deploy yellow 11") |
| queue / queued | docs/commands.md:4099 | pi follow-up and nextTurn messages held back: "so deliveries keep queueing" | internal: Job status `queued` (4312); StatusBar `Q queued` | loose | "held" for messages; keep `queued` for Jobs |
| event (pi) | docs/commands.md:3758 | pi's own extension events, lower-case: "`pi.events.emit("plannotator:request"" | upstream: pi events (`before_agent_start`, `agent_settled`) vs cpd Event | loose | "pi event" for pi's, "Event" for cpd's; the part mostly does |
| note | docs/commands.md:3757 | the text a User types in the Plan review dialog: "A note that is empty after trimming" | internal: TUI "No notes" (4319, transient lines); upstream: pi `ctx.ui.notify` notices (3737) | loose | "feedback text" for the dialog (the file is already `feedback-<review>.md`); "notice" for notify |
| **Fine** | n/a | Plan, Plan phase, Context record, Session (cpd), Event, Inbox, Inbox item, Cockpit, Engine, PR poll, PR post, Feedback item, User, Manager, Lead, Worker, Escalation, Gate, Quick check, Attempt, Job kind, Update Job, Spec (Worker Spec), Review Job, Lead Branch, Merged, Policy, Resource, Slot, Tenant, Skipped, Machine policy, MCP server, Attach, Resolved policy, Known owner | used in one sense each, or already qualified | none beyond the rows above | ok | none |

## PR occurrences

Unit: one hit of `\bprs?\b|pull[ _]requests?` (case-insensitive, 126 in the part). Count = hits in the row. "After the rename" is a proposal; the User decides contested names. Rows marked **agent message** are text the adapter shows or sends an agent: Event text, the Lead update and Manager digest, the reminder, refusals, and the Worker Spec rows. Every `mixed` row says why in its last cell. No `phase` or `step` use exists in the part (checked: `<PART> | rg -n -i 'phase'`; no `PR` phase, no `PR` step).

| Where (section or path:line range) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| docs/commands.md:3687 (§15.3) | 1 | pull request | "the pull request is open" | unchanged |
| docs/commands.md:3689 (§15.3) | 1 | object | "`cpd pr start <key>`" | `cpd workflow start <key>` (command noun renamed) |
| docs/commands.md:3690 (§15.3) | 1 | pull request | "`pr-feedback`" | unchanged (feedback on the pull request) |
| docs/commands.md:3690 (§15.3) | 2 | object | "Carries the PR's key in `data.pr`" | the Workflow's key in `data.workflow` |
| docs/commands.md:3692 (§15.3) | 1 | object | "`pr-started`" | `workflow-started` |
| docs/commands.md:3692 (§15.3) | 1 | mixed | "`pr-merged`" | User decides: Event of the pull request merge, or of the Workflow reaching `merged`? Both happen together |
| docs/commands.md:3692 (§15.3) | 1 | object | "a started PR's Jobs are now scheduled" | a started Workflow's Jobs |
| docs/commands.md:3692 (§15.3) | 2 | object | "a merged PR may start the PRs that need it" | a merged Workflow may start the Workflows that need it |
| docs/commands.md:3693 (§15.3) **agent message** | 1 | mixed | "`pr-base-changed`" | User decides: the `base` that changed is the Workflow's, and it follows the pull request's base on GitHub |
| docs/commands.md:3693 (§15.3) **agent message** | 1 | mixed | "the PR's `base` followed GitHub" | "the Workflow's `base` followed the pull request's base on GitHub" |
| docs/commands.md:3693 (§15.3) **agent message** | 1 | mixed | "as after its base PR merged" | "as after the Workflow it is based on merged" (the base is another Workflow, whose pull request merged) |
| docs/commands.md:3693 (§15.3) **agent message** | 1 | mixed | "the PR stays ready" | "the Workflow stays ready" if it means status; "the pull request stays ready to merge" if it means mergeability: the text does not say |
| docs/commands.md:3709 (§15.3) | 1 | pull request | "**`pr-feedback`, whatever its `reason`**" | unchanged |
| docs/commands.md:3709 (§15.3) | 1 | pull request | "PR #80's conflict notice" | unchanged (#80 is a GitHub number) |
| docs/commands.md:3841 (§15.7) **agent message** | 1 | mixed | "(3) the PRs of its Plan opened since" | "the Workflows of its Plan whose pull request opened since" |
| docs/commands.md:3841 (§15.7) **agent message** | 1 | mixed | "`PR <key> opened: <url>`" | `Workflow <key>: pull request opened <url>`: key is the Workflow's, url the pull request's |
| docs/commands.md:3841 (§15.7) | 1 | pull request | "new `pr-opened` Event of the Plan" | unchanged (a pull request was opened) |
| docs/commands.md:3846 (§15.7) **agent message** | 1 | mixed | "1 PR opened" | "1 pull request opened": counts pull requests |
| docs/commands.md:3850 (§15.7) **agent message** | 1 | pull request | "PR #207 opened" | unchanged |
| docs/commands.md:3852 (§15.7) **agent message** | 1 | mixed | "PRs:" | "Workflows:" if the lines are keyed by Workflow, "Pull requests:" if by number; the example line is keyed by key and url |
| docs/commands.md:3853 (§15.7) **agent message** | 1 | mixed | "PR rebase opened: https://github.com/earendil-works/cpd/pull/211" | as 3841: key of a Workflow plus URL of its pull request |
| docs/commands.md:3856 (§15.7) **agent message** | 1 | mixed | "\<n> PRs opened" | "\<n> pull requests opened" |
| docs/commands.md:3858 (§15.7) **agent message** | 1 | pull request | "2 PRs opened (#210, #211)" | unchanged (numbers) |
| docs/commands.md:3858 (§15.7) | 1 | pull request | "`pr-opened`" | unchanged |
| docs/commands.md:3858 (§15.7) | 1 | mixed | "`pr-merged`" | as 3692: Workflow and pull request merge together, the text does not say which the Event reports |
| docs/commands.md:3858 (§15.7) **agent message** | 1 | object | "PR mainfix 2 running, 1 queued" | Workflow mainfix 2 running, 1 queued |
| docs/commands.md:3858 (§15.7) **agent message** | 1 | mixed | "PR prlink open #207" | "Workflow prlink: pull request #207 open": key plus a pull request number and state |
| docs/commands.md:3870 (§15.7) **agent message** | 1 | pull request | "1 PR opened (#212)" | unchanged |
| docs/commands.md:3873 (§15.7) **agent message** | 1 | object | "PR mainfix 2 running, 1 queued" | Workflow mainfix ... |
| docs/commands.md:3873 (§15.7) **agent message** | 1 | mixed | "PR prlink open #207" | as 3858: a Workflow key with a pull request number and state |
| docs/commands.md:3874 (§15.7) **agent message** | 1 | object | "PR quiet-core 1 running" | Workflow quiet-core 1 running |
| docs/commands.md:3886 (§15.7) | 1 | pull request | "pr-opened" | unchanged |
| docs/commands.md:3886 (§15.7) | 1 | mixed | "pr-merged" | as 3692: Workflow and pull request merge together, the text does not say which the Event reports |
| docs/commands.md:3908 (§15.8) **agent message** | 1 | mixed | "`next pr`" | trigger phrase an agent writes: after the rename agents write "next workflow" or "next pull request"; User decides which phrases the heuristic keeps (may need both) |
| docs/commands.md:3911 (§15.8) **agent message** | 1 | object | "no Job key and no PR key of the Lead's Plan" | no Job key and no Workflow key |
| docs/commands.md:3918 (§15.8) **agent message** | 1 | object | "named no registered Job or PR" | "Job or Workflow" |
| docs/commands.md:3918 (§15.8) **agent message** | 1 | object | "or cpd pr add …" | `cpd workflow add …` |
| docs/commands.md:3923 (§15.8) | 1 | mixed | "`pr open` does not ask for a checked statement" | `pr open` acts on a Workflow and opens its pull request; name follows the command decision |
| docs/commands.md:4000 (§15.11) | 1 | other | "`gh pr checks --watch`" | other: GitHub CLI subcommand (`gh pr`); unchanged, upstream |
| docs/commands.md:4017 (§15.11) | 1 | other | "`gh pr checks --watch`" | other: GitHub CLI subcommand; unchanged, upstream |
| docs/commands.md:4023 (§15.12) | 1 | object | "run by the PR's `open` Job" | run by the Workflow's `open` Job |
| docs/commands.md:4023 (§15.12) | 1 | mixed | "`pr open` is its repair" | as 3923: the command acts on a Workflow and opens its pull request |
| docs/commands.md:4040 (§15.12) **agent message** | 1 | object | "run by the PR's open Job" | run by the Workflow's open Job |
| docs/commands.md:4111 (§15.16) | 1 | pull request | "only through a merged PR and the sync" | unchanged (a merged GitHub pull request) |
| docs/commands.md:4113 (§15.16) | 1 | object | "`prs.repo`" | `workflows.repo` |
| docs/commands.md:4137 (§15.16) **agent message** | 1 | pull request | "let a PR carry the change" | unchanged (agent-facing refusal text) |
| docs/commands.md:4238 (§15.20) | 1 | object | "in the PR's Lead worktree" | the Workflow's Lead worktree |
| docs/commands.md:4240 (§15.20) | 1 | mixed | "a later Job of this PR writes" | "this PR" is the pull request that carried this edit and also the Workflow of that Job; User decides, probably "this Plan's later Job" (history prose) |
| docs/commands.md:4252 (§16.1) **agent message** | 2 | object | "from the PR's policy" | the Workflow's policy (more exact: the repo's policy file); `protected.paths` of the Workflow's repo |
| docs/commands.md:4268 (§16.1) **agent message** | 2 | object | "the PR's Lead Branch against the PR's base" | the Workflow's Lead Branch against the Workflow's base |
| docs/commands.md:4268 (§16.1) **agent message** | 1 | mixed | "before `pr open`" | as 3923: the command acts on a Workflow and opens its pull request |
| docs/commands.md:4273 (§16.1) **agent message** | 1 | object | "brings the PR's Lead Branch up to date" | the Workflow's Lead Branch |
| docs/commands.md:4273 (§16.1) **agent message** | 1 | pull request | "opens the pull request itself" | unchanged |
| docs/commands.md:4273 (§16.1) **agent message** | 2 | pull request | "touch the pull request" | unchanged |
| docs/commands.md:4273 (§16.1) **agent message** | 1 | mixed | "the PR is not open, ask the Lead" | `update` refuses when the Workflow is not `open`, i.e. its pull request is not open: both; "the Workflow is not open" |
| docs/commands.md:4273 (§16.1) **agent message** | 1 | object | "name the PR's `open` Job" | the Workflow's `open` Job |
| docs/commands.md:4273 (§16.1) **agent message** | 1 | mixed | "not `pr open`" | as 3923: the command acts on a Workflow and opens its pull request |
| docs/commands.md:4299 (§17) | 1 | object | "a PR box title's Lead dot" | Workflow box title |
| docs/commands.md:4301 (§17) | 1 | pull request | "the open pull request read" | unchanged |
| docs/commands.md:4309 (§17) | 1 | mixed | "Job, Attempt, PR, Escalation and Plan review kinds" | the `pr-*` Event kinds are of both senses (see 4310) |
| docs/commands.md:4309 (§17) | 1 | pull request | "pull requests' checks for the dots" | unchanged |
| docs/commands.md:4310 (§17) | 2 | object | "its `prs`, each PR's key, repo, base, needs, status" | its `workflows`, each Workflow's key ... |
| docs/commands.md:4310 (§17) | 3 | pull request | "`pr`, `pull_request` (its `state`, for a closed pull request)" | unchanged: the `pr` field is `{number, url, opened_at}` (commands.md:837), a pull request; a clash with the rename, see Fix list |
| docs/commands.md:4310 (§17) | 1 | object | "for the PR boxes" | Workflow boxes |
| docs/commands.md:4310 (§17) | 1 | mixed | "the `pr-*` kinds" | kinds of both senses: `pr-started` object, `pr-feedback` pull request |
| docs/commands.md:4310 (§17) | 1 | pull request | "and `pr-feedback` included" | unchanged |
| docs/commands.md:4310 (§17) | 1 | object | "one box per PR that is not `cancelled`" | one box per Workflow |
| docs/commands.md:4310 (§17) | 2 | pull request | "and each pull request's `url`" | unchanged (also the `mergeable` use in the same cell) |
| docs/commands.md:4310 (§17) | 1 | object | "whether its PR has an update Job not ended" | whether its Workflow has an update Job not ended |
| docs/commands.md:4310 (§17) | 1 | pull request | "(only becoming `conflicting` appends `pr-feedback`)" | unchanged |
| docs/commands.md:4310 (§17) | 1 | pull request | "a `pull_requests` row changed" | unchanged |
| docs/commands.md:4311 (§17) | 1 | pull request | "an in-place label change of a `pr-ready` row" | unchanged (Inbox item kind for a pull request ready to merge) |
| docs/commands.md:4311 (§17) | 1 | pull request | "The authored pull request rule" | unchanged |
| docs/commands.md:4311 (§17) | 1 | pull request | "no `pull_requests` rows" | unchanged |
| docs/commands.md:4311 (§17) | 1 | mixed | "No PRs, engine, slots, Escalations or Events" | the column shows no Workflows and no pull requests; the earlier amendment 17 says it showed "the PRs of the Plans" |
| docs/commands.md:4325 (§17) | 1 | pull request | "the open pull request read" | unchanged |
| docs/commands.md:4337 (§17) | 1 | pull request | "the `pull_requests` mark" | unchanged |
| docs/commands.md:4341 (§17) | 1 | pull request | "`max(changed_at)` of `pull_requests`" | unchanged |
| docs/commands.md:4343 (§17) | 1 | mixed | "beside the state of a PR whose Lead works" | a box (Workflow) with a state word (pull request): "beside the pull request state of a Workflow whose Lead works" |
| docs/commands.md:4349 (§17) | 1 | object | "a PR box title's Lead dot" | Workflow box title |
| docs/commands.md:4350 (§17) | 1 | mixed | "a clean PR's merge runs" | "clean" and "merge" are pull request facts, the Lead dot belongs to the Workflow: "a Workflow whose pull request merges" |
| docs/commands.md:4356 (§17) | 1 | other | "Plan pr-links" | other: id of a past Plan (history, kept) |
| docs/commands.md:4370 (§17) | 1 | mixed | "PRs" | a section title in the Deploy column counting pull requests or Workflows (amendment 17); title text is user-visible: User decides "Workflows" or "Pull Requests" |
| docs/commands.md:4381 (§17) | 1 | pull request | "**The PR state word rule**" | unchanged (state words draft/open/closed/merged are the pull request's) |
| docs/commands.md:4381 (§17) | 1 | pull request | "when the PR is open and a draft" | unchanged |
| docs/commands.md:4381 (§17) | 1 | pull request | "when the PR is open and `conflicting`" | unchanged |
| docs/commands.md:4381 (§17) | 1 | object | "The plan view's PR box title" | Workflow box title |
| docs/commands.md:4383 (§17) | 1 | pull request | "pull request checks" | unchanged |
| docs/commands.md:4385 (§17) | 1 | object | "The plan view's PR box title" | Workflow box title |
| docs/commands.md:4398 (§17) | 1 | pull request | "`pr-ready`" | unchanged |
| docs/commands.md:4403 (§17) | 2 | pull request | "`pr-ready` items of the Plan's pull requests" | unchanged |
| docs/commands.md:4403 (§17) | 1 | pull request | "Cyan and White `pr-ready` items fill no Session dot" | unchanged |
| docs/commands.md:4403 (§17) | 1 | mixed | "`pr-*`" | kinds of both senses (4310) |
| docs/commands.md:4414 (§17) | 1 | pull request | "a click on the rest of a `pr-ready` row" | unchanged |
| docs/commands.md:4414 (§17) | 1 | pull request | "`cpd pr merge <owner>/<repo>#<n>`" | unchanged |
| docs/commands.md:4414 (§17) | 1 | pull request | "`cpd pr poll`" | unchanged |
| docs/commands.md:4414 (§17) | 1 | pull request | "`cpd pr post --pr <key>" | `cpd pr post` unchanged |
| docs/commands.md:4414 (§17) | 1 | object | "--pr \<key> --plan \<plan>" | `--workflow <key>` |
| docs/commands.md:4424 (§18) | 2 | object | "each repo of the Plan is a PR, added with `pr add`" | each repo of the Plan is a Workflow, added with `workflow add` |
| docs/commands.md:4432 (§18) | 2 | mixed | "the Gate runs only at `pr open` and `pr rebase`" | as 3923: the command acts on a Workflow and opens its pull request |
| docs/commands.md:4433 (§18) | 1 | pull request | "the review is the PR's own, on GitHub" | unchanged |
| docs/commands.md:4445 (§19) | 1 | object | "(PR `followups`)" | (Workflow `followups`) |
| docs/commands.md:4464 (§19) | 2 | pull request | "`pr poll` stores the PR's title" | unchanged |
| docs/commands.md:4465 (§19) | 1 | mixed | "the PRs of the Plans in the cockpit" | as 4311: Workflow or pull request, not stated |
| docs/commands.md:4467 (§19) | 1 | mixed | "the Plan review or the PR" | the Workflow or its pull request: unclear |
| docs/commands.md:4486 (§20) | 1 | other | "Job `contract-gate-pr`" | other: name of a past Job (history, kept) |
| docs/commands.md:4486 (§20) | 2 | mixed | "only at `pr open` and `pr rebase`" | as 3923: the command acts on a Workflow and opens its pull request |
| docs/commands.md:4486 (§20) | 1 | pull request | "the review is the PR's own, on GitHub" | unchanged |
| docs/commands.md:4496 (§20) | 2 | mixed | "refusals of `pr open` and `pr rebase`" | as 3923: the command acts on a Workflow and opens its pull request |
| docs/commands.md:4499 (§20) | 1 | mixed | "§7.6 `pr rebase` steps 1–4" | as 3923: the command acts on a Workflow and opens its pull request |
| docs/commands.md:4504 (§20) | 1 | mixed | "`run/<plan>/logs/pr.pre-merge.log`" | file name of the Gate's log at `pr open`; a path, so identifier (rename?) |

**Per section** (hits): §15.3 16, §15.7 19, §15.8 5, §15.11 2, §15.12 3, §15.16 3, §15.20 2, §16.1 12, §17 46, §18 5, §19 5, §20 8.

**Identifiers** (not in the 126; each cited at its line):

| Where | Identifier | Sense | After the rename |
|---|---|---|---|
| docs/commands.md:3689 | `pr_start_failed` | object | `workflow_start_failed` |
| docs/commands.md:3876 | pr_start_failed x1 | object | `workflow_start_failed` |
| docs/commands.md:4273 | `pr_title_required` | pull request | unchanged |
| docs/commands.md:4273 | `pr_closed` | pull request | unchanged |
| docs/commands.md:4381 | `pub fn pr_look(facts | pull request | unchanged |
| docs/commands.md:4381 | `palette::PR_CLOSED_ASKED` | pull request | unchanged |
| docs/commands.md:4381 | `palette::pr_open(clean)` | pull request | unchanged |
| docs/commands.md:4381 | `palette::pr_state(state)` | pull request | unchanged |
| docs/commands.md:4464 | `plans.pr_title` | pull request | unchanged |
| docs/commands.md:4464 | `pr_review_decision` | pull request | unchanged |
| docs/commands.md:4464 | `pr_checks` | pull request | unchanged |

Of these, `pr_start_failed` is the only object identifier (the Event `pr-started` and the flag `--pr` are in the table above).

## Fix list

**Rule R1 (mechanical, no row each).** Every `object` hit becomes Workflow: the noun in prose, the CLI noun (`pr add`, `pr start`), the flag `--pr <key>`, the JSON keys (`data.pr`, `prs`, `jobs.pr`), the table `prs` and the Event kinds `pr-started`, `pr-merged` (if the User decides it is of the object), and the code `pr_start_failed`. Counts of `object` hits per section: §15.3 7, §15.7 3, §15.8 3, §15.12 2, §15.16 1, §15.20 1, §16.1 6, §17 10, §18 2, §19 1. Total 36.

**Rule R2 (mechanical).** Every `pull request` hit stays `PR`, a pull request or a `pull_requests` identifier: no change. Total 47.

**Rule R3 (mechanical).** The two `other` hits for `gh pr checks --watch` stay (upstream GitHub CLI). The Plan id `pr-links` and the Job `contract-gate-pr` are history and stay.

Rows below need a judgement (`mixed`, stale, wrong, loose, overloaded). Each quote is found at its line.

| path:line | what to change | why |
|---|---|---|
| docs/commands.md:3687 | "The `job-merged` of an `open` Job": Reword; the Event is named merged but reports an opened pull request. Decide whether `open` Jobs get their own Event kind | wrong statement (verdict: wrong) |
| docs/commands.md:3689 | "`cpd pr start <key>`": Rule R1: command, flag, Event and code of the object are renamed (see rule below) | mechanical, counted per section below |
| docs/commands.md:3690 | "Carries the PR's key in `data.pr`": One Event, two senses: `pr-feedback` is the pull request's, `data.pr` is the Workflow's key. Decide the Event's name and the JSON key together (`data.workflow`) | mixed in one record |
| docs/commands.md:3692 | "`pr-merged`": Decide which merge the Event reports (the pull request's, which also moves the Workflow to `merged`) and name it | mixed |
| docs/commands.md:3693 | "`pr-base-changed`": Four mixed uses in one row: Workflow `base` follows the pull request's base on GitHub; "base PR" is another Workflow; "stays ready" has no clear subject. Reword with both nouns | mixed (agent-facing) |
| docs/commands.md:3841 | "(3) the PRs of its Plan opened since": Rewrite the message format of the Lead update: `PR <key> opened: <url>` -> Workflow key plus pull request URL; summary `1 PR opened` -> `1 pull request opened` | mixed (agent-facing) |
| docs/commands.md:3853 | "PR rebase opened: https://github.com/earendil-works/cpd/pull/211": Example line of the same format: key of a Workflow plus URL of a pull request; fix with 3841 | mixed (agent-facing) |
| docs/commands.md:3856 | "\<n> PRs opened": Summary of the collapsed row counts pull requests: `<n> pull requests opened` | mixed (agent-facing) |
| docs/commands.md:3858 | "PR prlink open #207": Manager digest line mixes a Workflow key with a pull request number and state; propose "Workflow prlink: pull request #207 open"; line 3873 repeats it | mixed (agent-facing) |
| docs/commands.md:3908 | "`next pr`": Trigger phrase of the reminder heuristic: decide whether it becomes `next workflow`, `next pull request` or both; this section says the list "is the whole definition" | mixed (changes behaviour) |
| docs/commands.md:3923 | "`pr open` does not ask for a checked statement": `pr open` acts on a Workflow and opens its pull request: name follows the command decision. Same word at 4023, 4268, 4273, 4432, 4486, 4496, 4499 (rows below) | mixed |
| docs/commands.md:4023 | "`pr open` is its repair": as 3923 | mixed |
| docs/commands.md:4240 | "a later Job of this PR writes": "this PR" is this pull request and the Workflow of that Job; history prose, reword with "this Plan" | mixed |
| docs/commands.md:4251 | "(was "Workflow")": Stale once Workflow returns with a new meaning; drop or reword as "cpd 1 called a Plan a workflow" | stale |
| docs/commands.md:4268 | "before `pr open`": as 3923 | mixed |
| docs/commands.md:4273 | "not `pr open`": as 3923; also "the PR is not open" (the Workflow's `update` refusal, said of its pull request): name both | mixed |
| docs/commands.md:4309 | "Job, Attempt, PR, Escalation and Plan review kinds": `PR` here stands for the Event kinds `pr-*`, which belong to both senses; list them by kind | mixed |
| docs/commands.md:4310 | "`pr`, `pull_request` (its `state`": JSON clash: field `pr` of `plan show` is the pull request's `{number, url, opened_at}` (commands.md:837) while `jobs.pr` and `--pr` name the object; after the rename `pr` stays with the pull request only if the object's field is renamed too | mixed in one record |
| docs/commands.md:4310 | "the `pr-*` kinds": kinds of both senses | mixed |
| docs/commands.md:4311 | "No PRs, engine, slots, Escalations or Events": Says the column shows no PRs; amendment 17 (4465) says it shows "the PRs of the Plans". Contradiction in the same part: decide which is true, then name the noun | mixed + wrong |
| docs/commands.md:4341 | "The **Dispatch column** follows `session-state` Events": Stale name: the same column is "Deploy column" at 4311, 4301, 4319 and 4365; also "Dispatch row" at 4381 and 4385 | stale |
| docs/commands.md:4343 | "beside the state of a PR whose Lead works": a state word of the pull request beside a Workflow's Lead dot | mixed |
| docs/commands.md:4350 | "a clean PR's merge runs": "clean" and "merge" are pull request facts; the Lead dot belongs to the Workflow | mixed |
| docs/commands.md:4370 | "PRs": User-visible section title in the Deploy column: `Workflows` or `Pull Requests`; User decides | mixed (visible text) |
| docs/commands.md:4403 | "`pr-*`": kinds of both senses | mixed |
| docs/commands.md:4414 | "`cpd pr post --pr <key>": One command, two senses: `pr post` posts to the pull request, `--pr \<key>` names the Workflow; after the rename `cpd pr post --workflow \<key>` | mixed in one command |
| docs/commands.md:4432 | "the Gate runs only at `pr open` and `pr rebase`": as 3923 (history table, cpd 2 column) | mixed |
| docs/commands.md:4465 | "the PRs of the Plans in the cockpit": amendment 17 (a row of history); see 4311 | mixed |
| docs/commands.md:4467 | "the Plan review or the PR": unclear subject: Workflow or pull request | mixed |
| docs/commands.md:4486 | "only at `pr open` and `pr rebase`": as 3923 (also 4496, 4499) | mixed |
| docs/commands.md:4504 | "`run/<plan>/logs/pr.pre-merge.log`": a file name that names the Gate's log of `pr open` and `pr rebase`: decide whether the path is renamed | mixed |
| docs/commands.md:3694 | "opens the blocking reason `escalation:<id>`": An agent-kind reason does not make a Session `blocked` (3727): "blocking reason" is wrong for it; propose "state reason" | wrong |
| docs/commands.md:3982 | "A Manager or a Lead never blocks": "never waits in its turn": `blocked` is a Session state for the human | loose |
| docs/commands.md:4087 | "owner Plan `pi-theme`": "originating Plan": owner is a Policy term | loose |
| docs/commands.md:4113 | "the directory of a Plan's repo, `prs.repo`": A Plan has one repo per PR (R34): "a Workflow's repo, `workflows.repo`" | wrong |
| docs/commands.md:4238 | "for a Plan's branch": A Plan has no branch; the Lead Branch belongs to the PR: "a Workflow's Lead Branch" | wrong |
| docs/commands.md:4202 | "(brief, from the spec)": "short"; "brief" and "spec" are CONTEXT terms (Brief, Spec) | loose |
| docs/commands.md:4252 | "from the PR's policy": "the repo policy file" (CONTEXT: Repo policy file) | loose |
| docs/commands.md:4405 | "Rows are ordered first by dot-urgency tier": Tier is the model strength of a Session; the dots and Inbox use "tier" for urgency | overloaded |
| docs/commands.md:4381 | "when the PR is open and a draft": Write "draft pull request"; Draft Job is cpd's | loose |
| docs/commands.md:4424 | "`workflow init`": cpd 1 cell stays; the cpd 2 cell is `plan new` ... "each repo of the Plan is a PR, added with `pr add`": Workflow | history, two meanings in one row |
| docs/commands.md:4311 | "`widgets` (Deploy column)": The Deploy column is neither the Deploy stage nor the `deploy` Inbox kind; User decides a name | overloaded |

## Counts

- Files audited: 1 (`docs/commands.md`), lines 3648 to 4512 = 865 lines, all read in full; file written: 1.
- Rows: `## Terms` 35 (plus one summary row), `## PR occurrences` 111 (plus 11 identifier rows), `## Fix list` 42 (plus 3 rules).
- PR word hits: 126. By sense: `object` 36, `pull request` 47, `mixed` 39, `other` 4, `phase` 0, `step` 0. Sum 126.
- Of the 126, 36 sit in agent-facing text (rows marked agent message).
- Identifier hits (outside the 126): 11 rows; `object` 2 (`pr_start_failed` at two lines), `pull request` 9.
- Verdicts in `## Terms`: loose 11, ok 3, overloaded 15, stale 2, wrong 4.
- Unverified: the sense of `pr-merged` and of "stays ready" (3692, 3693): the text does not say; coded `mixed`. Not run: any test or build (a read-only audit; the quick check is run before submit).
