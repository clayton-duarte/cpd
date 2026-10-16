# CPD 2

CPD (Context → Plan → Dispatch) turns one human ticket into a reviewed pull request by having a lead agent specify work and a swarm of agents execute it in parallel. This is cpd 2's vocabulary; the contract is [contract.md](docs/contract.md). cpd 1's vocabulary is the root `CONTEXT.md` of the commit before the switch-over (git history); every term below that is not marked new or changed means what it meant there.

## Phases

A Plan moves through the phases **grounding → plan → dispatch → pr → done** (contract §3.1).

**Grounding** (phase, new name):
The first phase of every Plan, in which the Lead grounds itself on the Plan's Context: it reads the Context's records and the material the User provides, receives Requirements, and interviews the User until unknowns are minimised. When the Lead is a fork of the Context's grounded Lead, it only adds what it learns and moves on. Replaces cpd 1's "Context" phase name, so that "Context" names only the entity.
_Avoid_: discovery, intake, context phase

**Plan** (phase):
The phase where the Lead turns Requirements into a draft graph of Jobs and the User approves it.
_Avoid_: breakdown, decomposition

**Dispatch**:
The phase that starts when the Plan is locked, in which PRs start, their Jobs are executed and merged, and each PR is opened after its Gate passes; it ends when every PR of the Plan is open.
_Avoid_: execution, run phase

**PR** (phase):
The phase from when every PR of the Plan is open until all are merged; PR feedback becomes appended Jobs. Then the Plan is **done**.
_Avoid_: review phase

## Work units

**Context** (entity):
The parent shared by two or more Plans: the shared knowledge between Plans that together make a product, optionally mapped to a Jira Epic. Not a session. Each machine keeps one grounded Lead per Context, which new Plans fork.
_Avoid_: project, folder, epic

**Context record**:
One typed, append-only entry of a Context's knowledge: a decision (which may supersede another), a note, a question, an answer, or a Plan. Written only through `cpd context …`.
_Avoid_: wiki page, memory

**Context store**:
A Context's records as append-only JSONL in a git repository at `~/.cpd/contexts/<id>/`, shared across the User's machines through a private remote, and indexed in the machine's database. Rendered as markdown for Leads on demand.
_Avoid_: wiki, notes folder

**Grounded Lead**:
The Lead session that holds a Context's grounding on one machine, forked once from the first Plan's Lead when it leaves the grounding phase. It is never typed into again; every new Plan of the Context starts as a fork of it.
_Avoid_: master session, template session

**Plan** (entity, new: replaces Workflow; changed by R34):
One deliverable over one or more PRs, optionally mapped to a Jira task, belonging to exactly one Context. Its Lead starts in the directory `plan new` runs from, not in a repository. cpd 1 called this a **Workflow**; the term Workflow is retired and every `workflow` command is now a `plan` command. Distinct from the Plan phase.
_Avoid_: workflow, task, project, run

**PR** (entity, new: R34):
The part of a Plan in one repository, tied to its Lead Branch: a repo, a base branch, a Lead Branch and Lead worktree, its Jobs, and the Pull Request opened from that branch. A PR may need other PRs of the same Plan to be merged before it starts; one repository may hold several PRs of one Plan. Distinct from the PR phase.
_Avoid_: sub-plan, track, slice, repo plan

**Job** (changed):
A self-contained unit of work defined by a Spec, executable by a fresh agent session with no other context. It belongs to exactly one PR of its Plan.
_Avoid_: card, task, step

**Spec**:
The complete, guess-free description of a Job: goal, context, files it touches, acceptance criteria and what is out of scope.
_Avoid_: brief, prompt, ticket

**Dependency**:
A relation saying a Job may only start after another Job of the same PR is Merged. Order across PRs is the PRs' needs.
_Avoid_: blocker, prerequisite

**Attempt**:
One execution of a Job by one fresh Worker session; a Job has one or more Attempts.
_Avoid_: retry, run

**Ledger** (changed):
The durable, append-only record of every Plan's Jobs, Dependencies, Attempts, Plan reviews, Escalations, Sessions and Events, with the before and after of every write. In cpd 2 it is one SQLite database per machine, written only by `cpd`.
_Avoid_: database, board, tracker

**Locked**:
The state of every Job once the Plan is locked: its Spec, Tier and Dependencies can no longer change; it can only be re-attempted or cancelled, and new Jobs can be appended.
_Avoid_: frozen, final

## Runtime

**Session** (new):
A pi session cpd knows about, with a role (Lead or Worker), a Context, usually a Plan, a tmux pane, and an agent state: working, blocked, idle or stopped.
_Avoid_: agent, pane, chat

**Event** (new: replaces the Lead queue):
An append-only record of something that happened, numbered in commit order. Every state change appends one. Events are the single channel to Leads, the TUI and agent states: the pi adapter follows them and delivers some to its Lead as messages, with or without a turn. They replace cpd 1's **Lead queue** (the per-Lead queue directory of message files) and the tmux options that carried agent states.
_Avoid_: message, notification, queue item

**Cockpit**:
The User's interface: one tmux window with global side bars and a centre bound to one Plan (plan view above, the Lead's chat below). tmux hosts the panes, ratatui draws the views.
_Avoid_: dashboard, TUI session

**Engine** (new):
The one `cpd watch` per machine that schedules every Plan's Jobs under one concurrency limit, watches liveness and timeouts, polls Pull Requests, and starts a PR once the PRs it needs are merged.
_Avoid_: daemon, scheduler, watcher per workflow

## Roles

**User**:
The human who owns the ticket and talks only to the Lead.
_Avoid_: operator, owner

**Lead**:
The agent session that translates User intent into Specs and orchestrates the Plan; it never writes code itself.
_Avoid_: coordinator, orchestrator, manager

**Worker**:
An agent session that executes one Attempt of one Job in its own worktree.
_Avoid_: subagent, executor, thread

**Reviewer** (removed): Removed by R19 (no Job-level Reviewer). The review is the Pull Request's own, on GitHub.

**Escalation**:
A question raised by an agent to its parent in the delegation tree (Worker → Lead → User) when continuing would require guessing.
_Avoid_: blocker, ask

## Outcomes

**Gate** (changed):
The repository's mechanical checks (tests, typecheck, lint), defined by the repo as its `pre-merge` hook in `.config/wt.toml` and run by cpd only at the PR level: on a PR's Lead Branch, in its Lead worktree, before `pr open` and `pr rebase` push. Never per Job or per merge (R19).
_Avoid_: CI, checks

**Quick check** (new):
The repository's `pre-commit` hook, run by cpd when a Worker submits; when the repo defines none, there is no quick check and nothing runs instead (R20).
_Avoid_: pre-flight, lint step

**Lead Branch** (changed):
The branch of one PR that accumulates its Merged Jobs and becomes its Pull Request; a Plan has one per PR.
_Avoid_: feature branch, integration branch

**Merged** (changed):
The terminal success state of a Job, and the outcome of its successful Attempt: the Attempt passed the quick check and the tamper check, rebased cleanly onto its PR's Lead Branch, and that Lead Branch was fast-forwarded to it.
_Avoid_: done, completed

**Tier**:
A two-character code naming the model and effort an agent runs at, e.g. `O3` for the Lead or `S1` for a coding Worker.
_Avoid_: level, profile
