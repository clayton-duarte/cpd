# CPD 2

## How to read this glossary

CPD (Continuous Parallel Delivery) turns one human ticket into a reviewed pull request by having a lead agent specify work and a swarm of agents execute it in parallel. This is cpd 2's vocabulary; the contract is [contract.md](docs/contract.md). cpd 1's vocabulary is the root `CONTEXT.md` of the commit before the switch-over (git history); every term below that is not marked new or changed means what it meant there, except **Workflow**, which returns with a new meaning (pro-d136).

The levels, the cpd objects and their roles. The levels are Jira's words, used as an analogy:

| Level (Jira analogy) | cpd object | Role |
|---|---|---|
| Epic | Context | Manager |
| Ticket | Plan | Lead |
| (none) | Workflow | Lead |
| Task | Job | Worker |

Transitional note: until the rename lands, the CLI (`cpd pr add`, `--pr`), the tables (`prs`, `pr_needs`), the Events and the docs still call a Workflow "PR", and the cockpit's third column is still titled Deploy; this glossary's words are the target. The rename deletes this note.

The one rule: **PR means only the pull request** on GitHub. Plan > Workflows > Jobs. History keeps the old words: ADRs, the archive, research, old Context records and applied migrations are never rewritten.

An entry is a bold term, a parenthesis with its kind and the Context record that decided it, a colon, the definition, and an `_Avoid_:` line when words are to be avoided. The kinds are entity, phase, stage, step, state, role, mechanism, policy, other and retired. A definition says what a thing is, never how a command works: the contract and [commands.md](docs/commands.md) own that, and an entry points to them. An Avoid word that is also a command is meant as a noun. A word with several senses has one entry per sense, each with a qualified name.

## Work units

**Context** (entity, pro-d136):
The parent shared by two or more Plans: the shared knowledge between Plans that together make a product, optionally mapped to a Jira Epic. Not a Session. Each machine may keep one Manager per Context. Distinct from the Context stage.
_Avoid_: project, folder; epic (Jira's word for the level, see the table)

**Context record**:
One typed, append-only entry of a Context's knowledge: a decision (which may supersede another), a note, a question, an answer, a Plan, or a Brief. Written only through `cpd context …`. An Axiom is not a Context record: it lives in AGENTS.md.
_Avoid_: wiki page, memory

**Context store**:
A Context's records, held only in the machine's database (tables `contexts` and `context_records`). Contexts are machine-local (hs-d23, hs-a1); an earlier git store was imported and archived by migration 0007. Rendered as markdown for Leads on demand.
_Avoid_: wiki, notes folder

**Distilled Context** (hs-d549, hs-d551; replacing hs-d194; the pilot of Epic B, auxiliary agents):
The curator at work: the distiller runs once per batch of uncurated records, and for each classifies it into a topic, compresses it into a short form (for machines), and proposes retirements (duplicate, superseded, documented records) and documentation candidates. The distiller writes curations and proposals (checked structurally by cpd), which the User approves through Inbox items. The records stay the truth. The Context is retrieved on demand with `cpd context search` (search over all records), `cpd context show --since <seq>` (records after a cursor), and `cpd context show --topic <t>` (curated records of one topic). The engine starts the distiller when a Context has 20 uncurated records or the newest is 10 minutes old; `cpd context distill` does it on demand. No first message holds a pushed digest or list of records (hs-d655): a Lead's first message is the header, the brief, and pointers to the on-demand commands; a Manager's is the header and pointers.
_Avoid_: summary, memory, compaction, cite-all, validation, model digest

**Grounded Lead** (retired, hs-d129):
No longer created (B3): the Manager replaced it, and migration 0010 archived every existing one. Formerly, the Lead session that holds a Context's grounding on one machine, forked once from the first Plan's Lead when it leaves the grounding phase. It is never typed into again; every new Plan of the Context starts as a fork of it.
_Avoid_: master session, template session

**Plan** (entity, pro-d136; changed from R34):
One deliverable over one or more Workflows, optionally mapped to a Jira ticket, belonging to exactly one Context. Its Lead starts in the directory `plan new` runs from, not in a repository. cpd 1 called this a Workflow (see Retired). Distinct from the Plan stage and from phase `plan`.
_Avoid_: project; task, run (as a noun)

**Workflow** (entity, pro-d136, pro-d138):
The part of a Plan in one repository: its repo, base branch, Lead Branch and Lead worktree, its Jobs, and the Workflows it needs. It ends in exactly one pull request, which cpd opens from its Lead Branch; it maps to one pull request but is not it. One repository may hold several Workflows of one Plan, and the Jira key of the work stays on the Workflow. cpd 1 used the word for a Plan (D3, retired); a GitHub Actions workflow is always written in full.
_Avoid_: PR, sub-plan, track, slice, repo plan

**Job** (changed):
A self-contained unit of work defined by a Spec, executable by a fresh agent session with no other context. It belongs to exactly one Workflow of its Plan, and has a Job kind. A GitHub Actions job is written in full.
_Avoid_: card, step; task (except in the Jira analogy of the table)

**Job kind** (new, hs-d340, hs-d341):
What a Job is for, and so which checks its submit runs: `work` (the default), `docs` (the contract amended first), `test` (red tests, written from the docs by a Worker other than the implementer), `impl` (makes the tests of the test Jobs it needs pass), `verify` (adds checks that change no product behaviour; the Lead adds it by hand), and `open` and `update`, which only cpd adds. The kinds a Workflow may hold follow its Shape. Defined once in contract §2.2 "Job kinds".
_Avoid_: type, category, role

**Open Job** (new, hs-d340):
The last Job of every started Workflow, added by cpd (engine E4), never by the Lead: a Job of kind `open` that is Ready when every other Job of its Workflow is merged or cancelled. Its Worker merges the base into the Lead Branch only if the two conflict; cpd then runs the Gate and opens the Workflow's pull request, ready, never a draft. `pr open` repairs a failed one or a down engine.
_Avoid_: opener, release Job

**Update Job** (new, hs-d340):
A Job of kind `update`, added by cpd for a Workflow whose pull request is already open and conflicts with its base (never for a base that merely moved, and never for feedback). Like the Open Job it merges the base, then cpd runs the Gate and pushes. `pr rebase` and `pr push` repair a failed one.
_Avoid_: sync Job, rebase Job

**Review Job** (new, B2):
A Job added with `job add --review`, whose Worker is a fork of the Plan's Lead, so it knows the intention. It depends on every Job of its Workflow that exists when it is added, reads the Lead Branch's diff against the base and the Plan's Specs, and writes its findings to `run/<plan>/reviews/<job>.md`. It commits nothing and ends Merged with no change to the Lead Branch. Optional: the Lead adds one only when it makes sense. It is not a Reviewer, and no Gate or verdict belongs to it; the Reviewing step names its place in the flow, and a person's look at the pull request is the pull request review.
_Avoid_: reviewer, verdict, code review Job

**Spec**:
The complete, guess-free description of a Job: its `goal`, `context` (what the Worker needs to know), the files it touches, acceptance criteria and what is out of scope. The field `context` is the Spec's own and is not the Context entity.
_Avoid_: brief (that is the Brief), prompt

**Worker Spec** (new):
The Spec as a Worker reads it: the Job's Spec rendered with the rows Who you are, Rules and Finish, and a section for the Job's kind (commands.md §16.1).
_Avoid_: prompt

**Dependency**:
A relation saying a Job may only start after another Job of the same Workflow is Merged. Order across Workflows is the Workflow needs.
_Avoid_: blocker, prerequisite

**Workflow need** (pro-d136):
A relation saying a Workflow starts only after another Workflow of the same Plan has merged its pull request (`--needs`). It orders Workflows as a Dependency orders Jobs. Stacking is the other order across Workflows.
_Avoid_: blocker, prerequisite

**Shape** (new, hs-d341, hs-d497):
How a Workflow lays out its Jobs, stored when the Workflow is added and never changed: `tdd` (docs, test, impl: the default), `docs` or `config` (no test Job), or `free` (every kind the Lead may add). It decides which Job kinds the Workflow may hold (contract §2.2 "Job kinds").
_Avoid_: template, type

**Stacked Workflow** (new, hs-d522, hs-d523):
A Workflow whose base is another Workflow's Lead Branch (`--base <key>`), so it is cut from that base Workflow's Lead Branch head and its pull request targets that branch. It starts when its base Workflow's pull request is open (contract §3.8 "Stacked PRs").
_Avoid_: dependent PR, child

**Lead Branch** (changed):
The branch of one Workflow that accumulates its Merged Jobs and becomes its pull request; a Plan has one per Workflow.
_Avoid_: feature branch, integration branch

**Lead worktree** (new):
The git worktree of a Workflow's Lead Branch, created when the Workflow starts. The Gate and the Open Job's merge run against it; Workers have their own worktrees.
_Avoid_: workflow worktree, main worktree

**Attempt**:
One execution of a Job by one fresh Worker session; a Job has one or more Attempts.
_Avoid_: retry, run (as nouns)

**Ledger** (changed):
The durable, append-only record of every Plan's Jobs, Dependencies, Attempts, Plan reviews, Escalations, Sessions and Events, with the before and after of every write. In cpd 2 it is one SQLite database per machine, written only by `cpd`.
_Avoid_: database, tracker; board (as a noun)

**Pending work** (new, hs-d159):
The in-flight work of a Context's active Plans: for every Plan not done and not archived, each Workflow not merged and not cancelled and its Jobs not cancelled and not yet on the Workflow's base (draft, queued, running, failed, or merged on the Lead Branch only), with titles and touched files. Read by `cpd context pending`; the Manager checks it before starting or routing work for an issue (commands.md §3.11, §15.7).
_Avoid_: backlog, WIP

## Job and Workflow statuses

**Draft** (state):
A Job the User has not approved yet. The engine never runs it, and it is editable and cancellable. Every Job is a draft in phase `plan`; in a running Plan the Lead adds its later batches as drafts (`job add --draft`), and the User approves each batch through a Plan review, which queues them. A Workflow with a draft Job never opens its pull request by itself: its Open Job waits for the draft. A draft pull request on GitHub is a different thing, and cpd opens none.
_Avoid_: proposed, pending Job

**Locked** (state):
The state of every Job that is not a draft: its title, Tier, Workflow, kind and Dependencies can no longer change, and its Spec can change only while the Job is queued and has no Attempt (`job edit --spec`, no Plan review; the old text stays in the history); it can otherwise only be re-attempted or cancelled, and new Jobs can be appended. A Job is Locked when the Plan is locked, or, if it was added as a draft later, when the review that covers it is approved. Distinct from the Plan lock (`plan lock`, the act that moves phase `plan` to `dispatch`) and from a file lock or kernel `flock` (Runtime).
_Avoid_: frozen, final

**Ready** (state):
A queued Job that the engine may start: every Dependency is Merged, its Workflow is started (status `dispatch` or `open`), and its Plan is in phase `dispatch` or `deploy`. An Open Job is Ready by its own rule (contract §3.2). A Ready Job that cannot start for want of a pool is Skipped. A Job that is not Ready is displayed as "held"; the status-line count of Ready Jobs reads "ready" (pro-d146).
_Avoid_: runnable, waiting (for a Job, use "held")

**Merged** (state, changed):
The terminal success state of a Job, and the outcome of its successful Attempt: the Attempt passed the quick check and the tamper check, rebased cleanly onto its Workflow's Lead Branch, and that Lead Branch was fast-forwarded to it. Not the word for a pull request: that is **squash-merged**, and the Workflow's status `merged` follows it.
_Avoid_: done (that is phase `done`), completed

**Skipped** (state, pro-d106):
A Ready Job that E3 could not start for want of a pool. Nothing is reserved for it; it keeps its place in the order urgent > skipped > queue, first skipped first served (contract §3.7 E3). Not a GitHub check conclusion.

**Workflow status** (state, pro-d144, pro-d146):
Where a Workflow is, stored as `waiting`, `dispatch`, `open`, `merged` or `cancelled` (contract §3.8). Qualified in prose: "a waiting Workflow" is one whose needs or base are not yet met; `dispatch` means started, its Jobs running, no pull request yet; `open` means its pull request is open; `merged` means its pull request is squash-merged; `cancelled` means given up. The words `dispatch` and `waiting` here are not the Dispatch stage and not a Session's waiting state.

**Escalation** (changed):
A question raised by an agent to its parent in the delegation tree (Worker → Lead → User) when continuing would require guessing.
_Avoid_: blocker; ask (as a noun)

## The CPD flow

**Phase, stage and step** (entity, pro-d145, pro-d146):
Three levels of one flow. A **phase** is the stored position of a Plan, one of five in this order: `grounding` → `plan` → `dispatch` → `deploy` → `done` (contract §3.1); a Plan in `deploy` or `done` returns to `dispatch` when a Workflow is added (`cpd pr add`). A **stage** is one of the three cockpit columns, read as past, present and future: Context, Plan, Dispatch. A **step** is a named piece of work the Lead or the Workers do inside a stage. An engine step is something else: the engine's own loop, written "engine step E7" (contract §3.7).
_Avoid_: stage for a phase, step for a Job

**Context stage** (stage, pro-d145):
The first stage: Grounding and Grilling, in phase `grounding`; it ends with `plan phase plan`. The Manager does it for the Context, each Lead for its Plan.

**Plan stage** (stage, pro-d145):
The second stage: Clarifying, Scoping, Splitting and Orchestrating, in phase `plan`; it ends with `plan lock`.

**Dispatch stage** (stage, pro-d145, pro-d146):
The third stage: Documenting, Red testing, Implementing, Verifying, Reviewing and Deploying, in phases `dispatch` and `deploy`. The Plan is `done` when every Workflow's pull request is merged. Its name is not the old phase "Dispatch" (retired) nor the Workflow status `dispatch`.

**Grounding** (step, pro-d146):
Reading the Context's records and the User's material (`context show`, `context note`).

**Grilling** (step, pro-d146):
The interview with the User, with `ask_user_question`, until unknowns are minimised (`context ask`, `context decide`).

**Clarifying** (step, pro-d146):
Restating the Requirements, writing the records, and listing the assumptions the User accepts (`context decide`, `context note`).

**Scoping** (step, pro-d146):
Choosing the repositories and cutting the deliverable into Workflows (`pr add`).

**Splitting** (step, pro-d146):
Writing the Jobs with their Specs, Dependencies and the Workflow needs (`job add`, `--needs`).

**Orchestrating** (step, pro-d146):
The Plan review, then the Plan lock (`plan render`, `cpd_plan_review`, `plan lock`).

**Documenting** (step, pro-d146):
Workers run the Jobs of kind `docs`; the engine runs them once the Plan is locked.

**Red testing** (step, pro-d146):
Workers run the Jobs of kind `test`: red tests that build and fail.

**Implementing** (step, pro-d146):
Workers run the Jobs of kind `impl` and `work`, and Merge them into the Lead Branch.

**Verifying** (step, pro-d146):
Workers run the Jobs of kind `verify`: checks that change no product behaviour.

**Reviewing** (step, pro-d146):
The optional Review Job only; there is no local review before the Gate (hs-d171). A person's review of the pull request is called pull request review, never Reviewing.
_Avoid_: pre-Gate review

**Deploying** (step, pro-d145, pro-d146):
The process of a Workflow's pull request, the last step once all its Jobs ran: the Open Job, the Gate (which runs inside it), opening the pull request, CI, pull request review and its polling (`pr poll`, `pr-feedback`), feedback fixes (Addressing), posting and merge.

**Addressing** (step):
A feedback fix is one `work` Job per cohesive fix, appended to the Workflow whose pull request is open (only `work` and review Jobs are allowed there); the Plan returns to phase `dispatch` and the Job's own merge pushes the Lead Branch to the pull request: no Update Job, no Gate at `gate.pr`. A conflict with the base queues an Update Job. `pr push` repairs a failed push; `pr rebase` is the other repair.

**Mechanical checks** (mechanism, changed hs-d340, hs-d341):
The checks cpd runs at an Attempt's submit, by the Job's kind: for a test Job the red tests (they build and fail), for an impl Job the same tests (they pass) and the red-test guard, and for an Open or Update Job the Lead Branch's fit with its base (merged only on a conflict) and the Gate, run when the Workflow's `gate.merge` is `local`; else the repo's CI. They are checks of the Job, so the Worker fixes what they report; the Lead runs none of them. `pr open` is the repair. Not a step: part of Deploying.

**Plan review** (entity, new):
The User's approval of the draft Jobs: one request, a summary in the Lead's chat and one dialog, covering every draft Job when the Plan is in phase `plan` (its approval is what `plan lock` needs) and the drafts of a running Plan batch by batch (contract §2.2 "Plan review").
_Avoid_: review (alone), approval

**CPD** (new, hs-d245):
Continuous Parallel Delivery. Its flow has three stages, Context, Plan and Dispatch: the cockpit's three columns, read as past, present and future. Each stage has steps; contract §3.9 maps them to phases and commands.

**φ** (new, hs-d694):
cpd's mark, as π is pi's: it overlaps the three letters C·P·D, references the golden ratio, and reads like a dango. Documents that name cpd's mark write it φ beside the name; the word `cpd` never changes. The mark and its logo are specified once in [docs/mark.md](docs/mark.md).
_Avoid_: cpd logo (the logo is defined soon)

## Runtime

**Session** (new):
A pi session cpd knows about, with a role (Lead, Worker or Manager), a Context, usually a Plan, a tmux pane, and an agent state: working, waiting (an agent waits for another agent: a Worker for its Lead's answer), blocked (the User has to act: a dialog, a pending Plan review), idle or stopped. A Session forked for a tangent the User starts (`cpd session fork`) is a sibling: drawn in the tree directly under its Context, after its Plans, never under a Plan or under the Session it came from (B1, B4). The other fork, a review Job's Worker, is hidden like every Worker.
_Avoid_: agent, pane, chat

**Event** (new: replaces the Lead queue):
An append-only record of something that happened, numbered in commit order. Every state change appends one. Events are the single channel to Leads, the TUI and agent states: the pi adapter follows them and delivers some to its Lead as messages, with or without a turn. They replace cpd 1's **Lead queue** (the per-Lead queue directory of message files) and the tmux options that carried agent states.
_Avoid_: message, notification, queue item

**Inbox** (new):
The single source of "needs you": the open Inbox items, shown by `cpd inbox` and the cockpit's Inbox section, opened and closed by the engine from their cause.
_Avoid_: notifications, alerts, to-do list

**Inbox item** (new):
One thing that needs the User (a dialog, a Plan review, an unread reply, a pull request of the User, a stuck Job, an engine error, an open question, or the engine being down), kept in `inbox_items` while its condition holds and forever after, closed; the User can snooze or dismiss it.
_Avoid_: notification, alert, task

**Cockpit**:
The User's interface: one tmux window with global side bars and a centre bound to one Plan (plan view above, the Lead's chat below). tmux hosts the panes, ratatui draws the views.
_Avoid_: dashboard, TUI session

**Engine** (new):
The one `cpd watch` per machine that schedules every Plan's Jobs under one concurrency limit, watches liveness and timeouts, polls Pull Requests, starts a PR once the PRs it needs are merged, and rebuilds stale Context digests.
_Avoid_: daemon, scheduler, watcher per workflow

**PR poll** (new, hs-d565):
The engine's poll, every 5 s, of every open pull request of the User on GitHub, by one search through `gh`. It stores them (table `pull_requests`), feeds the Dispatch column, and sends `pr-feedback` only to the Lead whose PR owns the pull request that needs attention (contract §7.6). A pull request a search misses is read directly and is never dropped on the search's absence (hs-d565). A Lead may run it by hand (`cpd pr poll`), and reads the detail with `cpd pr view`.
_Avoid_: PR watcher, PR sync

**Feedback item** (new, pro-d68):
One open thing a person said on a pull request a Plan owns: a review thread, an issue comment or a review with a body, not yet answered with a `cpd pr reply` (whose last line is the marker `<!-- cpd:reply -->`). The PR poll stores them and wakes the Lead (`pr-feedback`, reason `comments`); the Lead fixes or declines each and answers it; cpd never resolves a thread, and an open item blocks nothing. Defined once in `docs/contract.md` §2.2 "Feedback item".
_Avoid_: review comment (the word for what a reviewer wrote, not for the open thing cpd tracks), notification

**PR post** (new, pro-d64, pro-d65, pro-d66, pro-d117):
One message announcing a PR for review in Slack. The Lead calls `cpd pr post` when the PR is ready (open, green CI, no unaddressed bot comments) and the User says yes; one post per PR. The owner's policy configures the Slack channel, message template, and an optional emoji to react with after the merge. The post also turns on GitHub squash auto-merge (pro-d117): the PR merges when checks pass unless it is stacked (base PR not yet merged), in which case the request waits. Defined once in `docs/contract.md` §2.2 "PR post".
_Avoid_: Slack announcement, PR announcement

## Roles

**User**:
The human who owns the ticket and talks only to the Lead.
_Avoid_: operator, owner

**Manager** (new, B3):
The higher-level Lead of a Context, one live per Context per machine (`cpd context manager`). It works at the Epic level: holds the Context's records and Requirements, splits the User's requirements into Plans, starts their Leads with a Brief each, and knows where each is. Its memory is the Context records, not its chat. It acts only when the User talks to it (or a Lead messages it) and reads the Context on demand; at each turn start it receives a **Context update** with Plan Events since its last turn (created, phase, PR opened or merged, Job failed, done). It reads the full Context with `cpd context show` and searches it with `cpd context search`. Plan Events never wake it. Leads keep asking the User directly; the Manager never answers for the User. Default Tier `manager_tier` (O4).
_Avoid_: master, supervisor, orchestrator

**Brief** (new, B3):
A Context record of kind `brief`, written by `cpd plan new --brief-file`: what the Manager (or the User) asks of one Plan's Lead. The Lead's first message holds it in full; `cpd context show` and Context updates list it short (id and first line), and `cpd context show --record <id>` prints it whole. Not a Spec, which describes a Job.
_Avoid_: ticket, prompt

**Context update**:
The message the pi adapter adds at the start of a Manager's turn only (never a Lead's: Leads read the Context on demand with `cpd context show --since <seq>`, hs-d209), listing the Context records other Sessions wrote since its last one (commands.md §15.7). It ends with the **digest**: a `Plans:` section of the Plan Events (created, phase, PR opened or merged, Job failed, done) since then.
_Avoid_: notification, poll

**Lead**:
The agent session that translates User intent into Specs and orchestrates the Plan; it never writes code itself.
_Avoid_: coordinator, orchestrator

**Worker**:
An agent session that executes one Attempt of one Job in its own worktree.
_Avoid_: subagent, executor, thread

**Reviewer** (removed): Removed by R19 (no Job-level Reviewer). The review is the Pull Request's own, on GitHub.

## Outcomes

**Gate** (changed):
The repository's mechanical checks (tests, typecheck, lint), defined by the repo as its `pre-merge` hook in its repo policy file (`[hooks]`, contract §7.3) and run by cpd only at the PR level: on a PR's Lead Branch, in a gate worktree, at the submit of the PR's open or update Job (before cpd pushes), and in `pr open`, `pr push` and `pr rebase`, the repair, before they push, and only when the PR's Policy sets `pr.gate = "local"`; with cpd's default, `ci`, the repo's CI is the check (hs-d292). Never per Job (a Job's submit runs the quick check, and a test or impl Job's own tests) or per merge (R19).
_Avoid_: CI, checks

**Quick check** (new):
The repository's `pre-commit` hook, run by cpd when a Worker submits; when the repo defines none, there is no quick check and nothing runs instead (R20).
_Avoid_: pre-flight, lint step

**Tier**:
A two-character code naming the model and effort an agent runs at, e.g. `O3` for the Lead or `S1` for a coding Worker.
_Avoid_: level, profile

## Axioms

**Axiom** (new, hs-d607, hs-d610):
One of the cpd creator's core rules, applied everywhere and encoded into how cpd works and behaves: to every Context, owner, repo, role and tool. The one source is the global AGENTS.md of the pi that runs cpd (on hs `~/.pi/agent/AGENTS.md`, repo `clayton-duarte/pi-config`), which pi injects into every agent it launches without `--no-context-files`: the Manager, the Lead, the Worker and, since hs-d611, the Distiller. cpd reads no axiom: there is no config key and no record kind, and with no AGENTS.md cpd runs unchanged. An Axiom is not a decision (a dated choice about one Context's work, which a newer decision supersedes, hs-d358), not a note (a fact) and not a Policy (how one owner or repo works, per Scope, with typed keys). A decision, note, Policy, skill, Spec or doc may specialise an axiom but never restate or contradict it: it points to the axiom by its title. An axiom changes only when its creator edits the source.
_Avoid_: principle, convention, philosophy, rule

## Policies

**Policy** (changed, hs-d9, hs-d12):
How cpd and its agents work for one GitHub owner or repo: a fixed set of Domains, resolved per Policy key from cpd's built-in default and the instance's machine, owner and repo files under `$CPD_HOME/policy/` (contract §8.3). The machine file is optional and per-machine, never per owner or repo. Its `[mcp]` table, read only from the machine file, lists the MCP servers the machine may use (hs-d263, pro-d1; below). cpd reads nothing from a repo's tree. `cpd policy show` prints one. A Policy specialises the Axioms and never contradicts one.
_Avoid_: rules, settings, profile, config

**Domain** (new, hs-d288):
One typed slot of a Policy, owned by cpd's code: `pr`, `deploy`, `code`, `tests`, `monitor`, `instructions` or, for a repo only, `hooks`, `test-commands` or `protected`. Its value has typed keys that cpd's commands branch on (such as `pr.gate`, `local` or `ci`) and a free `text` for the agents its fixed audience names. A Scope that sets a Domain sets all of it; a key it leaves out takes cpd's default.
_Avoid_: section, category, setting

**Scope** (changed, hs-d9, hs-d12):
Where a Domain's value is set: cpd's default, a machine file, an owner file, or a repo file. The most specific Scope that sets a Domain wins (repo > owner > machine > default, hs-d279); no Domain adds up across Scopes. The machine Scope holds policies for one machine on which cpd runs, in the file `policy/host.toml`, and is optional: a missing machine file sets nothing. It is never part of a Policy key; a Policy resolves the same for every key on one machine and differently across machines.
_Avoid_: layer, level, override file

**Repo policy file** (new):
The repo scope of a Policy, `$CPD_HOME/policy/<owner>/<repo>.toml`: optional (a repo without one has no repo scope and inherits default, machine and owner; an empty file means the same), and the one place of the repo's hooks, test commands and protected paths (contract §8.3).


**Exposure** (new, hs-d9, hs-d12):
The set of Scopes from which a Domain may be set, named in the domain table of contract §8.3. An unexposed Domain in a Scope's file is `policy_invalid`. One key is exposed below its domain: the table `pr.post` is set in owner and repo files only, never in the machine file (pro-d126).
_Avoid_: visibility, permission

**Policy key** (new, hs-d288):
What a Policy is resolved for: an owner, or an owner and a repo name (`Owner(login) | Repo(login, name)`). cpd derives it from the one row a consumer acts on: a Context, its Manager and its Leads use the Context's owner; a PR, its Workers and the commands on it use the PR's repo. Never a machine, a path or a cwd.
_Avoid_: scope, selector, context

**Resource** (pro-d74, pro-d92):
Anything that has a limited number of slots available for concurrent use, declared in a policy file at host, owner or repo level (`[resources.<name>]`, contract §8.3 "The `resources` domain"), such as the one port an e2e run needs. A Job takes a slot through its Spec's `resources` and is its tenant from its first Attempt until it is merged or cancelled; a human takes one with `cpd resource run`, which holds a kernel `flock` released when the command dies. Nothing declared means no limit. The built-in `workers` is the host's Worker slots, its tenants the Jobs with a live Attempt (contract §3.4 "Tenancy").
_Avoid_: lock, mutex, semaphore

**Slot** (pro-d92):
The number of available concurrent units of a determined resource: one concurrent unit of it. Contract §1.4 "The resource locks and the holder record".
_Avoid_: lock, seat

**Tenant** (pro-d92, pro-d106):
The Job or user occupying a resource slot. A Job is a tenant from its first Attempt until it is merged or cancelled (a failed Job keeps all its resources); the Ledger records it, so it is derived by a join. A command's holder record names it (contract §1.4, §3.4 "Tenancy").

**Machine policy** (changed, pro-d1):
The machine Scope's file, `$CPD_HOME/policy/host.toml` (contract §8.3), which dotfiles render per machine: its `[model]` table first, then the machine's `cpd.policy` text (its `[mcp.servers.*]`). Besides Domains it may hold an `[mcp]` table, the MCP servers the machine may use, which only a machine file may have; `[mcp]` is not a Domain and takes no part in a Policy key.
_Avoid_: machine config

**MCP server** (new, pro-d1):
A remote server cpd may attach to a Session; listed in a Machine policy. Its tools reach a Session only through an explicit attach, which another Job documents. cpd holds no credential: pi's own OAuth store does.
_Avoid_: tool server, plugin

**Attach** (new, pro-d2, pro-d8):
Giving an MCP server to a Session: one explicit `cpd mcp attach <name>` (commands §24.1), for that Session only, not saved and never by default.
_Avoid_: enable, load, install

**Resolved policy** (changed, hs-d9, hs-d12):
The Policy of one Policy key: every Domain's value with its source (`default`, `machine:<path>`, `owner:<path>` or `repo:<path>`). It is total, because cpd's default sets every Domain.
_Avoid_: effective config, merged policy

**Known owner** (new, hs-d288):
An owner with a `policy/<owner>/owner.toml` file in `$CPD_HOME/policy/` (a file or a symlink that resolves). The known owners on this instance are `clayton-duarte`, `missionlane`, and `missionlane-scratch`, their files shipped by the User's dotfiles. Any other GitHub login is unknown until confirmed with `--confirm-fallback` on `context new --owner <login>` or `context owner <login>`. Confirming creates `policy/<owner>/owner.toml` as a relative symlink to `../clayton-duarte/owner.toml`. Deleting the link revokes the confirmation.
_Avoid_: registered, available

**Fallback** (new, hs-d288):
The owner whose policy is used when an unknown owner is confirmed: `clayton-duarte` on this instance. When confirming an unknown owner, `--confirm-fallback` creates a symlink to the fallback owner's policy file; if the fallback file does not exist, the command fails with `fallback_missing`.
_Avoid_: default owner, default policy
