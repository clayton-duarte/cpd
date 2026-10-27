# CPD 2

CPD (Continuous Parallel Delivery) turns one human ticket into a reviewed pull request by having a lead agent specify work and a swarm of agents execute it in parallel. This is cpd 2's vocabulary; the contract is [contract.md](docs/contract.md). cpd 1's vocabulary is the root `CONTEXT.md` of the commit before the switch-over (git history); every term below that is not marked new or changed means what it meant there.

The levels, the cpd objects and their roles:

| Level | cpd object | Role |
|---|---|---|
| Epic | Context | Manager |
| Ticket | Plan | Lead |
| Task | Job | Worker |

## Phases

A Plan moves through the phases **grounding → plan → dispatch → deploy → done** (contract §3.1). A Plan in **deploy** or **done** returns to **dispatch** when a PR is added (`cpd pr add`).

**Grounding** (phase, new name):
The first phase of every Plan, in which the Lead grounds itself on the Plan's Context: it reads the Context's records and the material the User provides, receives Requirements, and interviews the User until unknowns are minimised. A Lead always starts fresh, from its Plan's Brief and the Context's records (B3), given as the Context's digest and the records after it once the Context has one (Distilled Context). Replaces cpd 1's "Context" phase name, so that "Context" names only the entity.
_Avoid_: discovery, intake, context phase

**Plan** (phase):
The phase where the Lead turns Requirements into a draft graph of Jobs and the User approves it.
_Avoid_: breakdown, decomposition

**Dispatch**:
The phase that starts when the Plan is locked, in which PRs start, their Jobs are executed and merged, and each PR is opened by its open Job, whose submit runs the Gate; it ends when every PR of the Plan is open.
_Avoid_: execution, run phase

**PR** (phase):
The phase from when every PR of the Plan is open until all are merged; PR feedback becomes appended Jobs. Then the Plan is **done**; a PR added later (a follow-up to a bug or regression) moves it back to dispatch, and it is done again when that PR merges.
_Avoid_: review phase

## The CPD flow

**CPD** (new, hs-d245):
Continuous Parallel Delivery. Its flow has three stages, Context, Plan and Deploy: the cockpit's three columns, read as past, present and future. Each stage has steps; contract §3.9 maps them to phases and commands.

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

**Mechanical checks** (step, changed hs-d340, hs-d341):
The checks cpd runs at an Attempt's submit, by the Job's kind: for a test Job the red tests (they build and fail), for an impl Job the same tests (they pass) and the red-test guard, and for an open or update Job the Lead Branch's fit with its base (merged only on a conflict) and the Gate, run when the PR's `pr.gate` is `local`; else the repo's CI. They are checks of the Job, so the Worker fixes what they report; the Lead runs none of them. `pr open` is the repair.

**PR** (step):
The pull request is opened, by the PR's open Job's merge (`pr open` is the repair).

**Reviewing** (step, with Monitoring):
People, the User's review bot and CI review the open pull request; the engine polls it (`pr-feedback`). There is no local review before the Gate (hs-d171, supersedes hs-d77); a final review Job stays optional.
_Avoid_: pre-Gate review

**Addressing** (step):
A feedback fix is one `work` Job per cohesive fix, appended to the open PR (only `work` and review Jobs are allowed there); the Plan returns to Dispatching and the Job's own merge pushes the Lead Branch to the pull request: no update Job, no PR Gate. A conflict with the base queues an update Job. `pr push` repairs a failed push; `pr rebase` is the other repair.

**φ** (new, hs-d694):
cpd's mark, as π is pi's: it overlaps the three letters C·P·D, references the golden ratio, and reads like a dango. Documents that name cpd's mark write it φ beside the name; the word `cpd` never changes. The mark and its logo are specified once in [docs/mark.md](docs/mark.md).
_Avoid_: cpd logo (the logo is defined soon)

## Work units

**Context** (entity):
The parent shared by two or more Plans: the shared knowledge between Plans that together make a product, optionally mapped to a Jira Epic. Not a session. Each machine may keep one Manager per Context.
_Avoid_: project, folder, epic

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

**Plan** (entity, new: replaces Workflow; changed by R34):
One deliverable over one or more PRs, optionally mapped to a Jira task, belonging to exactly one Context. Its Lead starts in the directory `plan new` runs from, not in a repository. cpd 1 called this a **Workflow**; the term Workflow is retired and every `workflow` command is now a `plan` command. Distinct from the Plan phase.
_Avoid_: workflow, task, project, run

**PR** (entity, new: R34):
The part of a Plan in one repository, tied to its Lead Branch: a repo, a base branch, a Lead Branch and Lead worktree, its Jobs, and the Pull Request opened from that branch. A PR may need other PRs of the same Plan to be merged before it starts; one repository may hold several PRs of one Plan. Distinct from the PR phase.
_Avoid_: sub-plan, track, slice, repo plan

**Job** (changed):
A self-contained unit of work defined by a Spec, executable by a fresh agent session with no other context. It belongs to exactly one PR of its Plan, and has a kind.
_Avoid_: card, task, step

**Job kind** (new, hs-d340, hs-d341):
What a Job is for, and so which checks its submit runs: `work` (the default: any Job, as before), `docs` (the contract amended first), `test` (red tests, written from the docs by a Worker other than the implementer, with stubs so the branch builds; its Spec names its `tests` and the `test_files` that hold them), `impl` (makes the tests of the test Jobs it needs pass, and may not change their files unless the Lead approved the path with `job approve-tests`), `verify` (adds checks: end-to-end, smoke, load, contract; changes no product behaviour; a label like `docs`, with no Spec section of its own and no check at submit beyond the quick check; the Lead adds it by hand, it needs an `impl` Job of its PR, and a `tdd` or `free` PR allows it), and `open` and `update`, which only cpd adds. A PR's shape (`pr add --shape tdd|docs|config`, stored and enforced) lays out its kinds: `tdd` is docs, tests, impl; `docs` and `config` have no test Job. A repo needs `[test-commands]` in its repo policy file before its PRs can be `tdd`.
_Avoid_: type, category, role

**Open Job** (new, hs-d340):
The last Job of every started PR, added by cpd (engine E4), never by the Lead: a Job of kind `open` that is Ready when every other Job of its PR is merged or cancelled. Its Worker merges the PR's base into the Lead Branch only if the two conflict, resolves the conflicts and submits; cpd then runs the Gate in a test slot, pushes the Lead Branch and opens the pull request, ready, never a draft. A Gate failure fails the Job like any other. `pr open` is kept as the repair for a failed Job or a down engine.
_Avoid_: opener, release Job

**Update Job** (new, hs-d340):
A Job of kind `update`, added by cpd for a pull request that is already open and conflicts with its base (never for a base that merely moved, and never for feedback). Like the open Job it merges the base, then cpd runs the Gate and pushes. `pr rebase` and `pr push` are the repair.
_Avoid_: sync Job, rebase Job

**Spec**:
The complete, guess-free description of a Job: goal, context, files it touches, acceptance criteria and what is out of scope.
_Avoid_: brief, prompt, ticket

**Review Job** (new: B2):
A Job added with `job add --review`, whose Worker is a fork of the Plan's Lead, so it knows the intention. It depends on every Job of its PR that exists when it is added, reads the Lead Branch's diff against the base and the Plan's Specs, and writes its findings to `run/<plan>/reviews/<job>.md`. It commits nothing and ends Merged with no change to the Lead Branch; the Lead turns the findings into fix Jobs before the PR's open Job runs, so it registers a stub draft for them when it adds the review Job (a draft holds the open Job). Optional: the Lead adds one only when it makes sense. Not a Reviewer, and no Gate or verdict belongs to it.
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
A Job the User has not approved yet. The engine never runs it, and it is editable and cancellable. Every Job is a draft in the Plan phase; in a running Plan (any phase after it) the Lead adds its later batches as drafts (`job add --draft`), so that the Plan view shows every planned batch before it is approved, and the User approves each batch through a Plan review of the drafts, which queues them. A PR with a draft never opens by itself: its open Job waits for the draft.
_Avoid_: proposed, pending Job

**Locked**:
The state of every Job that is not a draft: its title, Tier, PR, kind and Dependencies can no longer change, and its Spec can change only while the Job is queued and has no Attempt, when the Lead corrects it with `job edit --spec` (no Plan review; the old text stays in the history); it can otherwise only be re-attempted or cancelled, and new Jobs can be appended. A Job is Locked when the Plan is locked, or, if it was added as a draft later, when the review that covers it is approved.
_Avoid_: frozen, final

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
One thing that needs the User (a dialog, a Plan review, an unread reply, a stuck Job, an engine error, an open question, or the engine being down), kept in `inbox_items` while its condition holds and forever after, closed; the User can snooze or dismiss it.
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

**PR post** (new, pro-d64, pro-d65, pro-d66):
One message announcing a PR for review in Slack. The Lead calls `cpd pr post` when the PR is ready (open, green CI, no unaddressed bot comments) and the User says yes; one post per PR. The owner's policy configures the Slack channel, message template, and an optional emoji to react with after the merge. Defined once in `docs/contract.md` §2.2 "PR post".
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

**Pending work** (new, hs-d159):
The in-flight work of a Context's active Plans: for every Plan not done and not archived, each PR not merged and not cancelled and its Jobs not cancelled and not yet on the PR's base (draft, queued, running, failed, or merged on the Lead Branch only), with titles and touched files. Read by `cpd context pending`; the Manager checks it before starting or routing work for an issue, and its digest summarises it (commands.md §3.11, §15.7).
_Avoid_: backlog, WIP

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

**Escalation**:
A question raised by an agent to its parent in the delegation tree (Worker → Lead → User) when continuing would require guessing.
_Avoid_: blocker, ask

## Outcomes

**Gate** (changed):
The repository's mechanical checks (tests, typecheck, lint), defined by the repo as its `pre-merge` hook in its repo policy file (`[hooks]`, contract §7.3) and run by cpd only at the PR level: on a PR's Lead Branch, in a gate worktree, at the submit of the PR's open or update Job (before cpd pushes), and in `pr open`, `pr push` and `pr rebase`, the repair, before they push, and only when the PR's Policy sets `pr.gate = "local"`; with cpd's default, `ci`, the repo's CI is the check (hs-d292). Never per Job (a Job's submit runs the quick check, and a test or impl Job's own tests) or per merge (R19).
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
The set of Scopes from which a Domain may be set, named in the domain table of contract §8.3. An unexposed Domain in a Scope's file is `policy_invalid`.
_Avoid_: visibility, permission

**Policy key** (new, hs-d288):
What a Policy is resolved for: an owner, or an owner and a repo name (`Owner(login) | Repo(login, name)`). cpd derives it from the one row a consumer acts on: a Context, its Manager and its Leads use the Context's owner; a PR, its Workers and the commands on it use the PR's repo. Never a machine, a path or a cwd.
_Avoid_: scope, selector, context

**Resource** (new, pro-d74):
A named exclusive thing of a repo, declared in its repo policy file (`[resources.<name>]`, contract §8.3 "The `resources` domain"), that one holder at a time may use, such as the one port an e2e run needs. A Job takes it through its Spec's `resources` and its Attempt holds it until it ends; a human takes it with `cpd resource run`. The lock is per repo on one machine, a kernel `flock` released when its holder dies. A repo that declares none has no lock and no limit.
_Avoid_: slot, lock, mutex, semaphore

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
