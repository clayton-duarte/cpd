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

The one rule: **PR means only the pull request** on GitHub. Plan > Workflows > Jobs. History keeps the old words: ADRs, the archive, research, old Context records and applied migrations are never rewritten.

An entry is a bold term, a parenthesis with its kind and the Context record that decided it, a colon, the definition, and an `_Avoid_:` line when words are to be avoided. The kinds are entity, phase, stage, step, state, role, mechanism, policy, other and retired. A definition says what a thing is, never how a command works: the contract and [commands.md](docs/commands.md) own that, and an entry points to them. An Avoid word that is also a command is meant as a noun. A word with several senses has one entry per sense, each with a qualified name.

## Work units

**Context** (entity, pro-d136):
The parent shared by two or more Plans: the shared knowledge between Plans that together make a product, optionally mapped to a Jira Epic. Not a Session. Each machine may keep one Manager per Context. Distinct from the Context stage.
_Avoid_: project, folder; epic (Jira's word for the level, see the table)

**Neighbour curation** (mechanism, Experimental Feature `write_curation`):
Checking a new Context record against its nearest in-force records at write time. It proposes pending retirements only; the User approves them. Contract §4.6.

**Vector cache** (entity):
`$CPD_HOME/vectors.db`: the derived file of record embeddings and already-checked markers for Neighbour curation. Each embedding row carries `content_hash`, the SHA-256 (hex) of the text it embedded, so `context embed` can skip a row whose text is unchanged. Written by `context curate` and `context embed`, and also read by `context search`. Not the source of truth and never synced; deleting it loses nothing. Contract §4.6.

**Permanent Context** (entity, pro-d338):
One of exactly three Contexts that every machine keeps: `cpd`, `host` (local maintenance of the machine) and `codereview` (the review intake of `inbox claim`). Never archived, renamed or deleted (`context_permanent`); its Plans behave as in any Context. The engine ensures all three every tick, and `context manager`, `inbox claim` and the host click ensure them too: a missing row is created and a live Manager is started, while a live Manager is left alone.
_Avoid_: default Context, system Context

**Context record**:
One typed, append-only entry of a Context's knowledge: a decision (which may supersede others: `--supersedes` may repeat, and prose that says a record is superseded without linking it is refused, contract §4.6), a note, a Plan, or a Brief. Written only through `cpd context …`. An Axiom is not a Context record: it lives in AGENTS.md.
_Avoid_: wiki page, memory

**Context store**:
A Context's records, held only in the machine's database (tables `contexts` and `context_records`). Contexts are machine-local (hs-d23, hs-a1); an earlier git store was imported and archived by migration 0007. Rendered as markdown for Leads on demand.
_Avoid_: wiki, notes folder

**Distilled Context** (hs-d549, hs-d551; replacing hs-d194; the pilot of Epic B, auxiliary agents):
The curator at work: the distiller runs once per batch of uncurated records, and for each classifies it into a topic, compresses it into a short form (for machines), and proposes retirements (duplicate, superseded, documented records). The distiller writes curations and proposals (checked structurally by cpd), which the User approves through Inbox items. The records stay the truth. The Context is retrieved on demand with `cpd context search` (in-process full-text search over all records, no engine to install), `cpd context show --since <seq>` (records after a cursor), and `cpd context show --topic <t>` (curated records of one topic). The engine starts the distiller when a Context has 20 uncurated records or the newest is 10 minutes old; `cpd context distill` does it on demand. No first message holds a pushed digest or list of records (hs-d655): a Lead's first message is the header, the brief, and pointers to the on-demand commands; a Manager's is the header and pointers.
_Avoid_: summary, memory, compaction, cite-all, validation, model digest

**Plan** (entity, pro-d136; changed from R34):
One deliverable over one or more Workflows, optionally mapped to a Jira ticket, belonging to exactly one Context. A Plan may be **Lead-less** in the phase `shaping`: its Manager creates it and explores through its explore Jobs before any Lead exists, with a null `lead_session` (contract §3.1). Otherwise its Lead starts in the directory `plan new` runs from, not in a repository. cpd 1 called this a Workflow (see Retired). Distinct from the Plan stage and from phase `plan`.
_Avoid_: project; task, run (as a noun)

**Workflow** (entity, pro-d136, pro-d138):
The part of a Plan in one repository: its repo, base branch, Lead Branch and Lead worktree, its Jobs, and the Workflows it needs. It ends in exactly one pull request, which cpd opens from its Lead Branch; it maps to one pull request but is not it. One repository may hold several Workflows of one Plan, and the Jira key of the work stays on the Workflow. cpd 1 used the word for a Plan (D3, retired); a GitHub Actions workflow is always written in full.
_Avoid_: PR, sub-plan, track, slice, repo plan

**Job** (changed):
A self-contained unit of work defined by a Spec, executable by a fresh agent session with no other context. It belongs to exactly one Workflow of its Plan, and has a Job kind. A GitHub Actions job is written in full.
_Avoid_: card, step; task (except in the Jira analogy of the table)

**Job kind** (new, hs-d340, hs-d341):
What a Job is for, and so which checks its submit runs: `work` (the default), `docs` (the contract amended first), `test` (red tests, written from the docs by a Worker other than the implementer), `impl` (makes the tests of the test Jobs it needs pass), `verify` (adds checks that change no product behaviour; the Lead adds it by hand), `open` and `update`, which only cpd adds, and the explore kinds `scout` and `research`, which the Lead adds to the Plan (see **Explore Job**). The kinds a Workflow may hold follow its Shape. Defined once in contract §2.2 "Job kinds".
_Avoid_: type, category, role

**Open Job** (new, hs-d340):
The last Job of every started Workflow, added by cpd (engine E4), never by the Lead: a Job of kind `open` that is Ready when every other Job of its Workflow is merged or cancelled. Its Worker merges the base into the Lead Branch only if the two conflict; cpd then runs the Gate and opens the Workflow's pull request, ready, never a draft. `pr open` repairs a failed one or a down engine.
_Avoid_: opener, release Job

**Update Job** (new, hs-d340):
A Job of kind `update`, added by cpd for a Workflow whose pull request is already open and conflicts with its base (never for a base that merely moved, and never for feedback). Like the Open Job it merges the base, then cpd runs the Gate and pushes. `pr rebase` and `pr push` repair a failed one.
_Avoid_: sync Job, rebase Job

**Scout** (new, pro-d210):
The Job kind `scout`: a Job of the explore kinds that answers one question from this machine only (where a checkout is, what a log or an Attempt transcript says, a code survey). Its deliverable is a Markdown artefact and a bounded summary, with no commit. Tier `H1`-`H3`. See contract §2.2 "Explore Jobs".
_Avoid_: explorer, survey Job

**Researcher** (new, pro-d215):
The Job kind `research`: an Explore Job that may also reach outside this machine (upstream docs, a registry, a web page, an attached MCP server) to answer one question. Its deliverable is the same as a Scout's. Tier `S1`-`S3`. An ordinary research Job of a docs Workflow, whose findings are committed as `docs/research/*.md`, is a `work` Job, not a Researcher.
_Avoid_: web Job, lookup Job

**Explore Job** (new, pro-d210, pro-d215):
A Job of kind Scout or Researcher. It belongs to the Plan, not to a Workflow (it has no PR), is queued at once with no draft, no review and no lock, and needs only other Explore Jobs. The Lead reads its summary when it merges; the artefact stays in the Plan's explore directory, and a Spec cites it by path. Defined in contract §2.2 "Explore Jobs".
_Avoid_: explore task, exploration, side Job

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
A Workflow whose base is another Workflow's Lead Branch (`--base <key>`), so it is cut from that base Workflow's Lead Branch head and its pull request targets that branch. It starts when its base Workflow's pull request is open (contract §3.8 "Stacked Workflows").
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

**SDK** (entity, hs-d802):
The whole of cpd as one programmable thing: the **core** (all behaviour and logic, the `cpd-core` library linked into every cpd process), the **ledger** (the one SQLite database per machine, `$CPD_HOME/cpd.db`, holding the typed rows and the append-only Event log) and the **engine** (the one `cpd watch` tick loop per machine). The interfaces (the CLI, the cockpit, the web UI) are its clients, not parts of it. Contract §1.1.
_Avoid_: platform, framework, API

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

**Addendum** (entity):
Information the Lead adds to a Job after its Spec is locked, stored beside the Spec, in no fingerprint, steered to a running Worker, and listed in the next Attempt's Spec; additive only (contract §3.4 "Addenda").
_Avoid_: edit (an edit changes the Spec: `job edit --spec`)

## The CPD flow

**Phase, stage and step** (entity, pro-d145, pro-d146):
Three levels of one flow. A **phase** is the stored position of a Plan, one of five in this order: `grounding` → `plan` → `dispatch` → `deploy` → `done` (contract §3.1); a Plan in `deploy` or `done` returns to `dispatch` when a Workflow is added (`workflow add`). A **stage** is one of the three cockpit columns, read as past, present and future: Context, Plan, Dispatch. A **step** is a named piece of work the Lead or the Workers do inside a stage. An engine step is something else: the engine's own loop, written "engine step E7" (contract §3.7).
_Avoid_: stage for a phase, step for a Job

**Context stage** (stage, pro-d145):
The first stage: Grounding and Grilling, in phase `grounding`; it ends with `plan phase plan`. The Manager does it for the Context, each Lead for its Plan.

**Plan stage** (stage, pro-d145):
The second stage: Clarifying, Scoping, Splitting and Orchestrating, in phase `plan`; it ends with `plan lock`.

**Dispatch stage** (stage, pro-d145, pro-d146):
The third stage: Documenting, Red testing, Implementing, Verifying and Deploying, in phases `dispatch` and `deploy`. The Plan is `done` when every Workflow's pull request is merged. Its name is not the old phase "Dispatch" (retired) nor the Workflow status `dispatch`.

**Grounding** (step, pro-d146):
Reading the Context's records and the User's material (`context show`, `context note`).

**Grilling** (step, pro-d146):
The interview with the User, with `ask_user_question`, until unknowns are minimised (`context decide`).

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

**Deploying** (step, pro-d145, pro-d146):
The process of a Workflow's pull request, the last step once all its Jobs ran: the Open Job, the Gate (which runs inside it), opening the pull request, CI, pull request review and its polling (`pr poll`, `pr-feedback`), feedback fixes (Addressing), posting and merge.

**Addressing** (step):
A feedback fix is one `work` Job per cohesive fix, appended to the Workflow whose pull request is open (only `work` Jobs are allowed there); the Plan returns to phase `dispatch` and the Job's own merge pushes the Lead Branch to the pull request: no Update Job, no Gate at `gate.pr`. A conflict with the base queues an Update Job. `pr push` repairs a failed push; `pr rebase` is the other repair.

**Plan review** (entity, new):
The User's approval of the draft Jobs: one request, a summary in the Lead's chat and one dialog, covering every draft Job when the Plan is in phase `plan` (its approval is what `plan lock` needs) and the drafts of a running Plan batch by batch (contract §2.2 "Plan review").
_Avoid_: review (alone), approval

**CPD** (new, hs-d245):
Continuous Parallel Delivery. Its flow has three stages, Context, Plan and Dispatch: the cockpit's three columns, read as past, present and future. Each stage has steps; contract §3.9 maps them to phases and commands.

## GitHub words

**pull request** (other, pro-d136):
GitHub's pull request. Each Workflow ends in exactly one, which cpd opens from the Workflow's Lead Branch, ready and never a draft (contract §7.6); a Workflow maps to it but is not it. Where CI exists, its checks run on it; a person reviews it (pull request review).
_Avoid_: PR (for the Workflow)

**PR** (other, pro-d136):
The abbreviation of pull request and nothing else. As a process it is the Deploying step, never the name of the Workflow.
_Avoid_: PR for a Workflow, a Plan or a phase

**draft pull request** (other, pro-d144):
GitHub's unfinished pull request. cpd opens none and no command makes one (contract §7.6). Not a draft Job (**Draft**, section 3).
_Avoid_: draft (alone, for a pull request)

**pull request review** (other, pro-d146):
What a person says on a pull request on GitHub: a review with a body or a verdict, its threads and comments. cpd reads it (the PR poll, Feedback items) and the Lead answers it; it is part of Deploying. Not the Reviewing step (retired, pro-d163).
_Avoid_: Reviewing, review (alone)

**squash-merged** (state, pro-d144):
What GitHub did to a pull request: it landed on the base (as one commit when the repo squash-merges; by the repo's merge commit or rebase otherwise). The Workflow's status `merged` follows it, and the Plan is `done` when every Workflow's pull request is. Not **Merged**, which is a Job's state.
_Avoid_: merged (for a pull request)

**GitHub Actions workflow** (other, pro-d136, pro-d143):
A workflow file of GitHub Actions, always written in full so that it is never taken for a Workflow. The policy key `[deploys]` `workflows` and the file `deploy-runs.json` name it today; the rename gives them `actions_workflows` and `actions_workflow`. A GitHub Actions job is written in full too.
_Avoid_: workflow (alone), deploy (for it)

**PR poll** (mechanism, hs-d565):
The engine's poll, every 5 s, of the open pull requests of the User on GitHub, one search through `gh`. It stores them (table `pull_requests`) and sends `pr-feedback` only to the Lead of the Workflow that owns the pull request needing attention (contract §3.7 E5, §7.6). A pull request the search misses is read directly and never dropped on the search's absence. `cpd pr poll` runs it by hand and `cpd pr view` reads the detail.
_Avoid_: PR watcher, PR sync

**Feedback item** (entity, pro-d68):
One open thing a person said on a pull request a Plan owns: a review thread, an issue comment or a review with a body, not yet answered with `cpd pr reply`. The PR poll stores them and wakes the Lead (`pr-feedback`, reason `comments`); cpd never resolves a thread. Defined in contract §2.2 "Feedback item".
_Avoid_: review comment (what a reviewer wrote, not the open thing cpd tracks), notification

**PR post** (entity, pro-d64, pro-d65, pro-d66, pro-d117):
One Slack message announcing a pull request for review, made by `cpd pr post`; one per pull request. The policy's `pr.post` keys set its channel, template and merged reaction. Defined in contract §2.2 "PR post".
_Avoid_: Slack announcement, PR announcement

**Jira status sync** (new, pro-d127):
For a PR that has its own Jira ticket (`--jira` on `pr add` or `pr edit`), cpd moves the ticket's status: to the policy's `pr.jira.in_progress` when the PR's first Attempt starts, to `pr.jira.in_review` when the PR is opened or adopted (the Workflow binds an open PR) or posted, (a ticket set on a Workflow also queues the target of its status),  and to `pr.jira.done` when it is recorded merged. The steps that reach those points only queue the move, each in its own transaction; engine E19 drains the queue through the pi helper `jira-transition.ts`, which calls the Atlassian MCP. Every run also assigns the ticket to the authenticated Atlassian user (pro-d454). A ticket never moves backwards, a target whose policy name is empty has no transition, and a move the ticket refuses is parked until the ticket or the policy changes. Specified in `docs/commands.md` §21.11 and §21.12.
_Avoid_: Jira update

## Roles

**User** (role):
The human who owns the ticket and talks only to the Lead (or the Manager).
_Avoid_: operator; owner (that is a GitHub login)

**Manager** (role, B3):
The higher-level Lead of a Context, one live per Context per machine (`cpd context manager`). It holds the Context's records and Requirements, splits them into Plans, starts each Plan's Lead with a Brief, and knows where each is. Its memory is the Context records, not its chat. It acts only when the User or a Lead talks to it, and reads the Context on demand; it never answers for the User. Its default Tier is `manager_tier`. A Permanent Context (`cpd`, `host`, `codereview`) always has one, ensured by the engine; its `cwd` is the User's home directory.
_Avoid_: master, supervisor, orchestrator

**Lead** (role):
The agent that translates User intent into Specs and orchestrates the Plan; it never writes code itself.
_Avoid_: coordinator, orchestrator

**Worker** (role):
An agent that executes one Attempt of one Job in its own worktree.
_Avoid_: subagent, executor, thread (as a word for a Worker)

**Agent key** (term, pro-d345, pro-d346):
The name of an agent in cpd's one hierarchy, Manager > Lead > Worker, mapped 1:1 to Context > Plan > Job: a Manager's key is its Context id (`cpd`), a Lead's is `<context>:<plan>` and a Worker's is `<context>:<plan>:<job>`, where every Attempt of a Job is that Job's Worker. The separator `:` is in no id pattern (`.` already separates `<plan>.<prefix>`). A fork is outside the hierarchy: it keeps its free name. The key is the Session's `name`, fixed for a Manager, a Lead that is not a fork and a Worker; only the `AgentKey` type writes it (contract §2.2 "Agent key").
_Avoid_: `<context>-manager`, a free-text name for a Lead or a Worker

**Worker runner** (term, pro-d165):
What runs a Worker: `pi` (a pi session in a tmux window) or `durable` (a headless child process of the Attempt's driver that resumes in place). Chosen by `durable_workers` in the `[experimental]` table of config.toml (contract §8.2), read through `Config::worker_runner()` when an Attempt is created and stored on it; both are kept.
_Avoid_: engine, backend

**Brief** (entity, B3):
A Context record of kind `brief`, written by `cpd plan new --brief-file`: what the Manager or the User asks of one Plan's Lead. Not a Spec, which describes a Job.
_Avoid_: ticket, prompt

**Tier** (other, pro-d146; Tier model v2, pro-d328 to pro-d332):
A code naming the model and effort an agent runs at: a letter (`G`, `H`, `S` or `O`, ranked G < H < S < O), then an effort digit `0`–`4` that the model offers, or `X` for a model with no effort levels; e.g. `O3` for a Lead, `S1` for a coding Worker, `GX` for a model with none. Effort 5 is never valid. Each role (auxiliar, worker, lead, manager) has a default Tier and a range of Tiers (contract §9). The model table is the policy's `[model]` domain. It means this only: the Inbox colours are called urgency.
_Avoid_: level, profile; tier (for a colour)

**Context update** (mechanism):
The message the pi adapter adds at the start of a Manager's turn only (never a Lead's), listing the Context records other Sessions wrote since its last one and the Plan Events since then (created, phase, pull request opened or merged, Job failed, done) (commands.md §15.7).
_Avoid_: notification, poll

## Runtime

**Session** (entity, pro-d146):
cpd's own record of an agent it runs: a role (Lead, Worker or Manager), a Context, usually a Plan, a tmux pane and an agent state: working, waiting (an agent waits for another agent), blocked (the User has to act), idle or stopped. A Session forked for a tangent (`cpd session fork`) is a sibling, drawn directly under its Context (B1, B4). Not a **tmux session** and not a **pi session**.
_Avoid_: agent, pane, chat

**tmux session** (other, pro-d146):
tmux's own session, the one that hosts the cockpit window and the panes. Always written in full.

**pi session** (other, pro-d146):
pi's own conversation file and process. A cpd Session wraps one; always written in full.

**Event** (entity):
An append-only record of something that happened, numbered in commit order; every state change appends one. Events are the single channel to Leads, the cockpit and agent states: the pi adapter follows them and delivers some to its Lead as messages. Not a GitHub event and not a tmux hook.
_Avoid_: message, notification, queue item

**Inbox** (entity):
The single source of "needs you": the open Inbox items, shown by `cpd inbox` and the cockpit's Inbox section, opened and closed by the engine from their cause. Its colours are urgency names.
_Avoid_: notifications, alerts, to-do list

**Inbox item** (entity):
One thing that needs the User (a dialog, a Plan review, an unread reply, a pull request of the User (labelled `review overdue` when its Slack post is 24 hours old and unreviewed; the User's RePost action sends the reminder), a stuck Job, an engine error, an open question, or the engine being down), kept in `inbox_items` while its condition holds and closed after; the User can snooze or dismiss it. Its kind `deploy` and the policy domain `deploy` keep that name; they are not the Deploying step.
_Avoid_: notification, alert, task (as a word for an item)

**cpd desktop** (entity, air-d10):
The Electron app in web/, the cockpit's desktop client (TUI and WebUI views); the name 'cpd app' is reserved for a possible future phone app.
_Avoid_: cpd app, Electron app

**Cockpit** (entity, pro-d145):
The User's interface: one tmux window with global side bars and a centre bound to one Plan (the plan view above, the Lead's chat below). Its three columns are the three stages, read as past, present and future: Context, Plan and the third column (Dispatch; its title in the code still differs until the rename lands). tmux hosts the panes, ratatui draws the views.
_Avoid_: dashboard, TUI session

**Engine** (entity):
The one `cpd watch` per machine: it schedules every Plan's Jobs under one concurrency limit, watches liveness and timeouts, polls pull requests, starts a Workflow once the Workflows it needs are merged, and curates Context records. Its loop is a list of engine steps.
_Avoid_: daemon, scheduler, one watcher per Workflow

**engine step** (mechanism, pro-d146):
One numbered part of the engine's loop, E1 to E17, always written "engine step E7" (contract §3.7). Not a step of the CPD flow.
_Avoid_: step (alone)

**Board post** (entity):
One message on a Plan's message board (`cpd board post`), read by the Plan's Sessions; written to a live Plan only.
_Avoid_: chat message, broadcast

**Resource** (entity, pro-d74, pro-d92):
Anything with a limited number of Slots for concurrent use, declared in a policy file (`[resources.<name>]`, contract §8.3 "The `resources` domain"), such as the one port an e2e run needs. A Job takes a Slot through its Spec's `resources` and is its Tenant from its first Attempt until it is Merged or cancelled; a person takes one with `cpd resource run`. Nothing declared means no limit. The built-in `workers` has the host's Worker Slots (contract §3.4 "Tenancy"). Not a file lock (see Locked, section 3).
_Avoid_: lock, mutex, semaphore

**Slot** (entity, pro-d92):
One concurrent unit of a Resource. Contract §1.4 "The resource locks and the holder record".
_Avoid_: lock, seat

**Tenant** (entity, pro-d92, pro-d106):
The Job or person occupying a Slot of a Resource. A Job is a Tenant from its first Attempt until it is Merged or cancelled (a failed Job keeps its Slots); the Ledger records it (contract §3.4 "Tenancy").

**MCP server** (entity, pro-d1):
A remote server cpd may attach to a Session, listed in the Machine policy. Its tools reach a Session only through an explicit Attach. cpd holds no credential: pi's own OAuth store does.
_Avoid_: tool server, plugin

**Attach** (mechanism, pro-d2, pro-d8):
Giving an MCP server to one Session, explicitly with `cpd mcp attach <name>` (commands.md §24.1): not saved and never by default. Not tmux's attach (`cpd tui --attach`).
_Avoid_: enable, load; install (as a word for it)

**Experimental Feature** (term, pro-d366, pro-d367):
A feature that is not permanent and can be turned on or off depending on how the User evaluates it. Its config lives only in the `[experimental]` table of config.toml, one bool key per feature (contract §8.2): `durable_workers` (the Worker runner), `write_curation`, `failure_triage` and `turn_question_guard`. The commands that show and set them are in commands.md §14.2b.
_Avoid_: flag, beta; toggle (for a setting that is permanent)

## Data

**SWR** (mechanism, pro-d440, pro-d446):
Stale-while-revalidate: cpd keeps the last confirmed upstream truth in SQLite, one engine poller revalidates it, every local reader reads SQLite, and every external write goes through one mutation outbox. Design: `docs/architecture/swr.md`; contract §4.4 and §4.7.
_Avoid_: cache (alone), sync

**query key** (term, pro-d446):
The name of one cached upstream read, `<system>:<kind>:<id>`, for example `github:pr:owner/repo#12`, `slack:channel:C123` or `jira:issue:EEC-1`. Its row is in `swr_queries`.
_Avoid_: cache key, id

**staleTime** (term, pro-d440):
How long a query stays fresh after its last confirmed change: a code constant per query rule (open pull request 5 s, repository, viewer and cloud id 24 h, Slack channel 30 min, models 7 d, a merged or closed pull request never). Freshness only: it never evicts a row.
_Avoid_: TTL, expiry

**gcTime** (term, pro-d440):
How long a row whose last observer has gone is kept before eviction: 7 days, config key `swr.gc_days`. A row with an open Workflow, an open Inbox item or an active mutation is never evicted.
_Avoid_: TTL, cache lifetime

**mutation outbox** (mechanism, pro-d440):
The one queue of every external write, `swr_mutations`: a CLI write enqueues it and returns its id with the optimistic state at once, and the engine drains it. Its statuses are `pending`, `in_flight`, `revalidating`, `done`, `failed` and `superseded`.
_Avoid_: queue, posting set, pending list

**overlay (effective state)** (mechanism, pro-d440):
The state a reader sees: the raw cached row with the patches of its active mutations applied in submission order. A failed mutation's overlay is dropped at once; a `revalidating` one is dropped when a fetch that started after the write's completion confirms it (the fence, `docs/architecture/swr.md` §10).
_Avoid_: optimistic cache, draft state

**endpoint health** (term, pro-d440):
The state of one external endpoint (`github`, `slack`, `jira`, `ollama.lab`, `bifrost`) in `swr_endpoints`: its last outcome, whether it is `idle`, `fetching` or `paused`, its outage and its rate limit. A failing endpoint raises one `error` Event per outage and never deletes cached data.
_Avoid_: uptime, ping, status page

## Checks and gates

**Gate** (mechanism, changed, pro-d146):
The repo's `pre-merge` hook, run by cpd on a Workflow's Lead Branch at the submit of its open or update Job, before cpd pushes, and by the repair commands `pr open`, `pr push` and `pr rebase`; only when the policy key `gate.pr` is `local` (cpd's default is `none`: core/src/policy/default.toml). It is never run per Job. Where `gate.pr` is `none`, CI is the check where it exists. Keys and hooks: contract §7.3, §8.3.
_Avoid_: CI; checks (GitHub's word); quick check (that is the `pre-commit` hook)

**Quick check** (mechanism, pro-d146):
The repo's `pre-commit` hook, run by cpd on a Worker's Attempt at its merge, only when the policy key `gate.merge` is `local` (default `none`). With no such hook there is no Quick check and nothing runs instead.
_Avoid_: pre-flight, lint step; Gate (that is `pre-merge`)

**Mechanical checks** (mechanism, changed hs-d340, hs-d341):
The checks cpd runs at an Attempt's submit, by the Job's kind: for a test Job the red tests (they build and fail), for an impl Job the same tests (they pass) and the red-test guard, and for an open or update Job the Lead Branch's fit with its base (refused only on a conflict) and the Gate. The Worker fixes what they report; the Lead runs none. Not a step of the CPD flow: part of Deploying.

**CI** (other):
The repo's GitHub checks on a pull request, where CI exists (this repo runs none). cpd's `pr.feedback.checks` key decides whether failing checks raise `pr-feedback`. Not the Gate and not Mechanical checks.
_Avoid_: checks (alone)

## Policy

**Policy** (policy, changed, hs-d9, hs-d12):
How cpd and its agents work for one GitHub owner or repo: a fixed set of Domains, resolved per Policy key from cpd's built-in default and the machine, owner and repo files under `$CPD_HOME/policy/` (contract §8.3). cpd reads nothing from a repo's tree. `cpd policy show` prints one. A Policy specialises the Axioms and never contradicts one.
_Avoid_: rules, settings, profile; config (as a word for a Policy)

**Domain** (policy, hs-d288):
One typed slot of a Policy, owned by cpd's code. The set and each Domain's keys, Exposure and audience are the table in contract §8.3; the domains `gate`, `pr`, `deploy` and `hooks` are not the Gate, the pull request, the Deploying step or git hooks of other tools. A Scope that sets a Domain sets all of it; a key it leaves out takes cpd's default.
_Avoid_: section, category, setting

**Scope** (policy, changed, hs-d9, hs-d12):
Where a Domain's value is set: cpd's default, a machine file, an owner file or a repo file. The most specific Scope that sets a Domain wins (repo > owner > machine > default, hs-d279); no Domain adds up across Scopes. A Scope is never part of a Policy key.
_Avoid_: layer, level, override file

**Policy key** (policy, hs-d288):
What a Policy is resolved for: an owner, or an owner and a repo name. cpd derives it from the one row a consumer acts on: a Context, its Manager and its Leads use the Context's owner; a Workflow, its Workers and the commands on it use the Workflow's repo. Never a machine, a path or a cwd.
_Avoid_: selector

**Exposure** (policy, hs-d9, hs-d12):
The set of Scopes from which a Domain may be set, named in the domain table of contract §8.3. An unexposed Domain in a Scope's file is `policy_invalid`; the table `pr.post` is exposed in owner and repo files only (pro-d126).
_Avoid_: visibility, permission

**Repo policy file** (policy):
The repo Scope of a Policy, `$CPD_HOME/policy/<owner>/<repo>.toml`: optional, and the one place of the repo's hooks, test commands and protected paths (contract §8.3).

**Machine policy** (policy, changed, pro-d1):
The machine Scope's file, `$CPD_HOME/policy/host.toml`, optional and per machine, never per owner or repo. Besides Domains it may hold the `[mcp]` table, the MCP servers the machine may use, which only a machine file may have; `[mcp]` is not a Domain.
_Avoid_: machine config

**Resolved policy** (policy, changed, hs-d9, hs-d12):
The Policy of one Policy key: every Domain's value with its source (`default`, `machine:<path>`, `owner:<path>` or `repo:<path>`). It is total, because cpd's default sets every Domain.
_Avoid_: effective config, merged policy

**Known owner** (policy, hs-d288):
An owner with a `policy/<owner>/owner.toml` file in `$CPD_HOME/policy/`. Any other GitHub login is unknown until confirmed with `--confirm-fallback` (contract §8.3 "Known owners"); `cpd policy show` prints which owner file a key resolves to.
_Avoid_: registered, available

**Fallback** (policy, hs-d288):
The owner whose policy file a confirmed unknown owner's link points to (contract §8.3); if that file does not exist, confirming fails with `fallback_missing`.
_Avoid_: default owner, default policy

**owner** (other, pro-d146):
A GitHub login, user or organisation: it names a Policy's owner scope and a repo's owner. It never means the User.
_Avoid_: owner (for the User)

## Axioms and marks

**Axiom** (policy, hs-d607, hs-d610):
One of the cpd creator's core rules, applied to every Context, owner, repo, role and tool. Its one source is the global AGENTS.md of the pi that runs cpd, which pi injects into every agent it launches; cpd reads no axiom and has no key or record kind for one. An Axiom is not a decision, not a note and not a Policy. A decision, note, Policy, skill, Spec or doc may specialise an Axiom but never restate or contradict it: it points to the Axiom by its title.
_Avoid_: principle, convention, philosophy; rule (as a word for an Axiom)

**φ** (new, hs-d694):
cpd's mark, as π is pi's: it overlaps the three letters C·P·D, references the golden ratio, and reads like a dango. Documents that name cpd's mark write it φ beside the name; the word `cpd` never changes. The mark and its logo are specified once in [docs/mark.md](docs/mark.md).
_Avoid_: cpd logo (the logo is defined soon)

## Retired words

Each line: what the word meant, what replaces it, the record that decided it. History keeps these words; live docs, skills and help text do not use them.

**Workflow (cpd 1)** (retired, pro-d136):
cpd 1's name for what is now a Plan. Retired by D3; the word **Workflow** returns with a new meaning (section 2).

**PR phase** (retired, pro-d145):
A phase named `pr`, once the last before `done`. The phase is now `deploy`; the process is the Deploying step.

**Deploy (stage)** (retired, pro-d145):
The third stage and the third cockpit column. Now the Dispatch stage; `deploy` stays the phase and the conclusion of Dispatch. The cockpit's third column is titled Deploy by the code until the rename lands. The policy domain `deploy` and the Inbox kind `deploy` keep their names.

**Reviewer** (retired, R19):
A Job-level reviewer. Removed: the review is the pull request review on GitHub, and the optional Review Job has no verdict.

**Grounded Lead** (retired, hs-d129):
The Lead Session that held a Context's grounding, forked for every new Plan. Replaced by the Manager (B3); migration 0010 archived every one.

**Sync Job** (retired, hs-d340):
A name for a Job that merges the base into the Lead Branch; never a Job kind. The Open Job and the Update Job do it.

**workflow worktree** (retired, pro-d136):
The git worktree of a Lead Branch. Now the **Lead worktree**; contract and code still use the old words until the rename.

**Clarifying (interview)** (retired, pro-d146):
The old step of the interview with the User. Now Grilling; the name Clarifying belongs to the Plan stage.

**Specifying** (retired, pro-d146):
The old step of writing Requirements, records and assumptions. Now Clarifying.

**Dividing** (retired, pro-d146):
The old step of cutting the deliverable. Now Scoping (the Workflows) and Splitting (the Jobs).

**Sequencing** (retired, pro-d146):
The old step of ordering the Jobs. Now Splitting.

**Approving** (retired, pro-d146):
The old step of the Plan review and lock. Now Orchestrating.

**Dispatching (step)** (retired, pro-d146):
The old step in which the engine runs the Jobs. Now the first steps of the Dispatch stage (Documenting, Red testing, Implementing, Verifying).

**Mechanical checks (step)** (retired, pro-d146):
The old step of checks before the pull request opens. Now part of Deploying; **Mechanical checks** stays a mechanism (section 8).

**Reviewing and Monitoring** (retired, pro-d146):
The old step of watching the pull request and its review. Now part of Deploying (the PR poll, pull request review).

**Review Job** (retired, pro-d163):
The optional Job added with `job add --review`, whose Worker forked the Plan's Lead. Retired: no Worker forks a Lead's session any more. Code review intake is the Plan's Lead's; PR verification uses `--kind verify` or `--shape`.

**Reviewing** (retired, pro-d163):
The optional step after Verifying, run by a Review Job. Retired with it: the Dispatch stage is Documenting, Red testing, Implementing, Verifying and Deploying.
