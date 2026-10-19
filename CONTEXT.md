# CPD 2

CPD (Context → Plan → Deploy) turns one human ticket into a reviewed pull request by having a lead agent specify work and a swarm of agents execute it in parallel. This is cpd 2's vocabulary; the contract is [contract.md](docs/contract.md). cpd 1's vocabulary is the root `CONTEXT.md` of the commit before the switch-over (git history); every term below that is not marked new or changed means what it meant there.

## Phases

A Plan moves through the phases **grounding → plan → dispatch → deploy → done** (contract §3.1). A Plan in **deploy** or **done** returns to **dispatch** when a PR is added (`cpd pr add`).

**Grounding** (phase, new name):
The first phase of every Plan, in which the Lead grounds itself on the Plan's Context: it reads the Context's records and the material the User provides, receives Requirements, and interviews the User until unknowns are minimised. A Lead always starts fresh, from its Plan's Brief and the Context's records (B3), given as the Context's digest and the records after it once the Context has one (Distilled Context). Replaces cpd 1's "Context" phase name, so that "Context" names only the entity.
_Avoid_: discovery, intake, context phase

**Plan** (phase):
The phase where the Lead turns Requirements into a draft graph of Jobs and the User approves it.
_Avoid_: breakdown, decomposition

**Dispatch**:
The phase that starts when the Plan is locked, in which PRs start, their Jobs are executed and merged, and each PR is opened after its Gate passes; it ends when every PR of the Plan is open.
_Avoid_: execution, run phase

**PR** (phase):
The phase from when every PR of the Plan is open until all are merged; PR feedback becomes appended Jobs. Then the Plan is **done**; a PR added later (a follow-up to a bug or regression) moves it back to dispatch, and it is done again when that PR merges.
_Avoid_: review phase

## The CPD flow

**CPD** (new, hs-n31):
Context, Plan, Deploy: the cockpit's three columns, read as past, present and future. Each stage has steps; contract §3.9 maps them to phases and commands.

**Context** (stage):
Grounding, Clarifying, Specifying; phase `grounding`, ending with `plan phase plan`. The Manager does it for the Context, each Lead for its Plan.

**Grounding** (step):
Reading the Context's records and the User's material.

**Clarifying** (step):
The interview, until unknowns are minimised.

**Specifying** (step):
Writing Requirements, assumptions and records.

**Plan** (stage):
Dividing, Sequencing, Approving, Dispatching; phases `plan` and `dispatch`.

**Dividing** (step):
Splitting the work into PRs and Jobs (`pr add`, `job add`).

**Sequencing** (step):
Ordering with `--needs` and PR needs: what runs in parallel, what in series.

**Approving** (step):
The Plan review (`plan render`, `cpd_plan_review`), then `plan lock`.

**Dispatching** (step):
The engine runs the Jobs.

**Deploy** (stage):
Mechanical checks, PR, Reviewing and Monitoring, Addressing; the end of phase `dispatch` and phase `deploy` until `done`.

**Mechanical checks** (step):
The Gate, run by `pr open`.

**PR** (step):
The pull request is opened.

**Reviewing** (step, with Monitoring):
People, the User's review bot and CI review the open pull request; the engine polls it (`pr-feedback`). There is no local review before the Gate (hs-d171, supersedes hs-d77); a final review Job stays optional.
_Avoid_: pre-Gate review

**Addressing** (step):
Feedback Jobs are appended, the Plan returns to Dispatching, then `pr rebase`.

## Work units

**Context** (entity):
The parent shared by two or more Plans: the shared knowledge between Plans that together make a product, optionally mapped to a Jira Epic. Not a session. Each machine may keep one Manager per Context.
_Avoid_: project, folder, epic

**Context record**:
One typed, append-only entry of a Context's knowledge: a decision (which may supersede another), a note, a question, an answer, a Plan, or a Brief. Written only through `cpd context …`.
_Avoid_: wiki page, memory

**Context store**:
A Context's records, held only in the machine's database (tables `contexts` and `context_records`). Contexts are machine-local (hs-d23, hs-a1); an earlier git store was imported and archived by migration 0007. Rendered as markdown for Leads on demand.
_Avoid_: wiki, notes folder

**Distilled Context** (new, hs-d194; the pilot of Epic B, auxiliary agents):
A compressed rendering of a Context's **records in force** (every decision, note, question and answer no decision supersedes), kept so that a Lead's or Manager's first message stays small. Stored as a **Context digest**, a file per Context under `run/context-digests/`, written by a one-shot pi run (the **distiller**, Tier `context_distill_tier`) and accepted only if it cites every record in force by its id, cites nothing else and is at most 32 KB. It is derived, never a Context record: the records stay the truth and the digest can be rebuilt at any time. The engine rebuilds it in the background once a Context's newest record is 10 minutes old; `cpd context distill` does it on demand, and `cpd context show --digest` prints it. A first message holds the digest plus the records written after it. Not the Manager's turn-start digest of Plan Events (below).
_Avoid_: summary, memory, compaction

**Grounded Lead** (retired, hs-d129):
No longer created (B3): the Manager replaced it, and migration 0010 archived every existing one. Formerly, the Lead session that holds a Context's grounding on one machine, forked once from the first Plan's Lead when it leaves the grounding phase. It is never typed into again; every new Plan of the Context starts as a fork of it.
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

**Review Job** (new: B2):
A Job added with `job add --review`, whose Worker is a fork of the Plan's Lead, so it knows the intention. It depends on every Job of its PR that exists when it is added, reads the Lead Branch's diff against the base and the Plan's Specs, and writes its findings to `run/<plan>/reviews/<job>.md`. It commits nothing and ends Merged with no change to the Lead Branch; the Lead turns the findings into fix Jobs before `pr open`. Optional: the Lead adds one only when it makes sense. Not a Reviewer, and no Gate or verdict belongs to it.
_Avoid_: reviewer, verdict, code review Job

**Dependency**:
A relation saying a Job may only start after another Job of the same PR is Merged. Order across PRs is the PRs' needs.
_Avoid_: blocker, prerequisite

**Attempt**:
One execution of a Job by one fresh Worker session; a Job has one or more Attempts.
_Avoid_: retry, run

**Ledger** (changed):
The durable, append-only record of every Plan's Jobs, Dependencies, Attempts, Plan reviews, Escalations, Sessions and Events, with the before and after of every write. In cpd 2 it is one SQLite database per machine, written only by `cpd`.
_Avoid_: database, board, tracker

**Draft**:
A Job the User has not approved yet. The engine never runs it, and it is editable and cancellable. Every Job is a draft in the Plan phase; in a running Plan (any phase after it) the Lead adds its later batches as drafts (`job add --draft`), so that the Plan view shows every planned batch before it is approved, and the User approves each batch through a Plan review of the drafts, which queues them. A PR with a draft never opens by itself.
_Avoid_: proposed, pending Job

**Locked**:
The state of every Job that is not a draft: its Spec, Tier and Dependencies can no longer change; it can only be re-attempted or cancelled, and new Jobs can be appended. A Job is Locked when the Plan is locked, or, if it was added as a draft later, when the review that covers it is approved.
_Avoid_: frozen, final

## Runtime

**Session** (new):
A pi session cpd knows about, with a role (Lead, Worker or Manager), a Context, usually a Plan, a tmux pane, and an agent state: working, blocked, idle or stopped. A Session forked for a tangent the User starts (`cpd session fork`) is a sibling: drawn in the tree directly under its Context, after its Plans, never under a Plan or under the Session it came from (B1, B4). The other fork, a review Job's Worker, is hidden like every Worker.
_Avoid_: agent, pane, chat

**Event** (new: replaces the Lead queue):
An append-only record of something that happened, numbered in commit order. Every state change appends one. Events are the single channel to Leads, the TUI and agent states: the pi adapter follows them and delivers some to its Lead as messages, with or without a turn. They replace cpd 1's **Lead queue** (the per-Lead queue directory of message files) and the tmux options that carried agent states.
_Avoid_: message, notification, queue item

**Inbox** (new):
The single source of "needs you": the open Inbox items, shown by `cpd inbox` and the cockpit's Inbox section, opened and closed by the engine from their cause.
_Avoid_: notifications, alerts, to-do list

**Inbox item** (new):
One thing that needs the User (a dialog, a Plan review, an unread reply, a stuck Job, an engine error, an open question, or the engine being down), kept in `inbox_items` while its condition holds and forever after, closed; the User can snooze or dismiss it.
_Avoid_: notification, alert, task

**Cockpit**:
The User's interface: one tmux window with global side bars and a centre bound to one Plan (plan view above, the Lead's chat below). tmux hosts the panes, ratatui draws the views.
_Avoid_: dashboard, TUI session

**Engine** (new):
The one `cpd watch` per machine that schedules every Plan's Jobs under one concurrency limit, watches liveness and timeouts, polls Pull Requests, starts a PR once the PRs it needs are merged, and rebuilds stale Context digests.
_Avoid_: daemon, scheduler, watcher per workflow

**PR poll** (new):
The engine's poll, every 5 s, of every open pull request of the User on GitHub, by one search through `gh`. It stores them (table `pull_requests`), feeds the Dispatch column, and sends `pr-feedback` only to the Lead whose PR owns the pull request that needs attention (contract §7.6). A Lead may run it by hand (`cpd pr poll`), and reads the detail with `cpd pr view`.
_Avoid_: PR watcher, PR sync

## Roles

**User**:
The human who owns the ticket and talks only to the Lead.
_Avoid_: operator, owner

**Manager** (new, B3):
The higher-level Lead of a Context, one live per Context per machine (`cpd context manager`). It works at the Epic level: holds the Context's records and Requirements, splits the User's requirements into Plans, starts their Leads with a Brief each, and knows where each is. Its memory is the Context records, not its chat. It acts only when the User talks to it (or a Lead messages it) and receives a **Context update** with a **digest** of its Plans at each turn start; Plan Events never wake it. Leads keep asking the User directly; the Manager never answers for the User. Default Tier `manager_tier` (O5).
_Avoid_: master, supervisor, orchestrator

**Brief** (new, B3):
A Context record of kind `brief`, written by `cpd plan new --brief-file`: what the Manager (or the User) asks of one Plan's Lead. The Lead's first message holds it in full; `cpd context show` and Context updates list it short (id and first line), and `cpd context show --record <id>` prints it whole. Not a Spec, which describes a Job.
_Avoid_: ticket, prompt

**Pending work** (new, hs-d159):
The in-flight work of a Context's active Plans: for every Plan not done and not archived, each PR not merged and not cancelled and its Jobs not cancelled and not yet on the PR's base (draft, queued, running, failed, or merged on the Lead Branch only), with titles and touched files. Read by `cpd context pending`; the Manager checks it before starting or routing work for an issue, and its digest summarises it (commands.md §3.11, §15.7).
_Avoid_: backlog, WIP

**Context update**:
The message the pi adapter adds at the start of a Manager's turn only (never a Lead's: Leads read the Context on demand with `cpd context show --since <seq>`, hs-d209), listing the Context records other Sessions wrote since its last one (commands.md §15.7). It ends with the **digest**: a `Plans:` section of the Plan Events (created, phase, PR opened or merged, Job failed, Escalation, done) since then.
_Avoid_: notification, poll

**Lead**:
The agent session that translates User intent into Specs and orchestrates the Plan; it never writes code itself.
_Avoid_: coordinator, orchestrator

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
