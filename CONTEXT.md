# CPD

CPD (Context → Plan → Dispatch) turns one human ticket into a reviewed pull request by having a lead agent specify work and a swarm of agents execute it in parallel.

## Phases

**Context**:
The phase where the Lead grounds itself in a Context (the entity below) and the material the User provides, receives Requirements, and interviews the User until unknowns are minimised. If the Context already exists, grounding is skipped or appended to.
_Avoid_: discovery, intake

**Plan** (phase):
The phase where the Lead turns Requirements into a draft graph of Jobs and the User approves it.
_Avoid_: breakdown, decomposition

**Dispatch**:
The phase that starts at Plan approval, in which Jobs are executed, reviewed and merged, ending when the Pull Request is done.
_Avoid_: execution, run phase

## Work units

**Context** (entity):
The parent shared by two or more Leads: the shared knowledge between Plans that together make a product, optionally mapped to a Jira Epic. Not a session. Distinct from the Context phase.
_Avoid_: project, folder, epic

**Plan** (entity):
Something actionable and a deliverable on its own: today's Workflow, one repo and one Pull Request, optionally mapped to a Jira task, belonging to a Context. Distinct from the Plan phase.
_Avoid_: task, project

**Context store**:
The typed records of a Context (decisions, notes, questions with their answers, its Plans), written by `cpd context …` and rendered as markdown for Leads; a git repository at `~/.cpd/contexts/<slug>/`.
_Avoid_: wiki, notes folder

**Cockpit**:
The User's interface: one tmux window with global side bars and a centre bound to one Plan (plan view above, the Lead's chat below). tmux hosts the panes, ratatui draws the views.
_Avoid_: dashboard, TUI session

**Workflow**:
One run of CPD for one ticket in one repository, producing one Pull Request.
_Avoid_: task, project, run

**Job**:
A self-contained unit of work defined by a Spec, executable by a fresh agent session with no other context.
_Avoid_: card, task, step

**Spec**:
The complete, guess-free description of a Job: goal, context, files it touches, acceptance criteria and what is out of scope.
_Avoid_: brief, prompt, ticket

**Dependency**:
A relation saying a Job may only start after another Job is Merged.
_Avoid_: blocker, prerequisite

**Attempt**:
One execution of a Job by one fresh Worker session; a Job has one or more Attempts.
_Avoid_: retry, run

**Ledger**:
The durable, append-only record of a Workflow's Jobs, Dependencies, Attempts, reviews and Escalations.
_Avoid_: database, board, tracker

**Locked**:
The state of every Job once Dispatch starts: its Spec, Tier and Dependencies can no longer change; it can only be re-attempted or cancelled, and new Jobs can be appended.
_Avoid_: frozen, final

## Roles

**User**:
The human who owns the ticket and talks only to the Lead.
_Avoid_: operator, owner

**Lead**:
The agent session that translates User intent into Specs and orchestrates the Workflow; it never writes code itself.
_Avoid_: coordinator, orchestrator, manager

**Worker**:
An agent session that executes one Attempt of one Job in its own worktree.
_Avoid_: subagent, executor, thread

**Reviewer**:
A read-only agent session, forked from the Lead, that judges one submitted Attempt against its Spec.
_Avoid_: checker, verifier

**Escalation**:
A question raised by an agent to its parent in the delegation tree (Worker → Lead → User) when continuing would require guessing.
_Avoid_: blocker, ask

## Outcomes

**Gate**:
The repository's mechanical checks (tests, typecheck, lint), normally run through its `verify` script.
_Avoid_: CI, checks

**Lead Branch**:
The branch that accumulates Merged Jobs and becomes the Pull Request.
_Avoid_: feature branch, integration branch

**Merged**:
The terminal success state of a Job: an Attempt passed the Gate, was approved by a Reviewer, and its commits were integrated into the Lead Branch.
_Avoid_: done, completed

**Tier**:
A two-character code naming the model and effort an agent runs at, e.g. `O3` for the Lead or `S1` for a coding Worker.
_Avoid_: level, profile
