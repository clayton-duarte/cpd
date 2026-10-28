# Glossary audit: skills, adapter text and what agents are told

Part `aud-pi` of the Workflow rename audit (Context records pro-d136, pro-d138). Report only: no file but this one was changed. Paths are relative to the repo root. Sense codes are the six of the Spec: `object`, `pull request`, `phase`, `step`, `mixed`, `other`. "After the rename" assumes the User's decision: `object` becomes **Workflow**, `pull request` keeps "PR" or is written in full; this is a proposal, the User decides.

## Coverage

| Path | Lines | Read |
|---|---|---|
| `pi/skills/cpd-lead/SKILL.md` | 1-399 | in full (two reads: 1-309, 310-399) |
| `pi/skills/cpd-lead/references/pr-description.md` | 1-104 | in full |
| `pi/skills/cpd-lead/references/questions.md` | 1-58 | in full |
| `pi/skills/cpd-lead/references/spec-files.md` | 1-104 | in full |
| `pi/skills/cpd-manager/SKILL.md` | 1-143 | in full |
| `core/src/spec/worker.md` | 1-223 | in full (the Worker Spec; the open and update Job Specs are its `kind == "open"` / `"update"` branches, there is no separate file) |
| `core/src/policy/default.toml` | 1-54 | in full |
| `core/src/context/distill_prompt.md` | 1-28 | in full |
| `pi/extension.ts` | 1-7 | in full (no agent-facing string) |
| `pi/lib/*.ts` (56 files, 5321 lines) | all | skimmed: only the hit lines of the string search below were read in place, plus `reminder.ts` 1-32 and `updates.ts` 196-240 in full; no file was read top to bottom |

Not read, and why: `pi/test/*` (out of scope except pinned vocabulary, below), `pi/compact-tools`, `pi/ask-user-question` (out of scope), `pi/lib/pill-spec.json` (not text agents read), the Rust sources under `core/src` other than the three text files (the Spec limits Rust to `.md` and `.toml` text; the commands compile other strings, which belong to another part). `rg -l --glob '*.md' --glob '*.toml' . core/src` found only `core/src/policy/default.toml`, `core/src/spec/worker.md` and `core/src/context/distill_prompt.md`; `core/src/spec/` also holds `mod.rs` and `resources_tests.rs` (not text). `CONTEXT.md` and `docs/commands.md` were read only to check a claim (cited where used).

Commands behind each count (run from the repo root):

- Agent-facing strings of the adapter: `cd pi; rg -n '"[^"]{20,}"|`[^`]{20,}`' lib extension.ts` gives 563 hit lines; read in place by filtering out code-only hits. Most hits are error messages for the log, not shown to an agent; the ones with agent or User text are `reminder.ts:8`, `mainguard.ts:8`, `testguard.ts:7`, `noblock.ts:11`, `askguard.ts:8,36`, `pnguard.ts:16`, `secretguard.ts:21-22`, `hostapprove.ts:21,53`, `plan-review.ts:67-77`, `review.ts:245`, `state.ts` labels, `updates.ts` lines 187-302 and `delivery.ts:35-43`.
- PR hits in the adapter: `cd pi; rg -n -i '\bPRs?\b|pull request|\bpr[-_ ]|\bworkflow' lib extension.ts` gives 56 lines in 10 files (`updates.ts` 33, `reminder.ts` 5, `slack-parse.ts` 6, `delivery.ts` 2, `event.ts` 3, `turns.ts` 3, `testguard.ts` 1, `slack-post.ts` 1, `noblock.ts` 1, `mainguard.ts` 1). `workflow` has no hit.
- PR tokens in the skills (`cd pi/skills`): prose tokens `\bPRs?\b|\b[Pp]ull [Rr]equests?\b` (case-sensitive `PR`); command forms `--pr\b`, `cpd pr <verb>`, `gh pr <verb>`, `pr.<key>`, `` `pr-<event>` ``, `` `pr_<code>` ``, `<pr-key>`, `pr-description.md`. Raw totals: Lead 251 prose + 168 command tokens, Manager 42 + 10, `pr-description.md` 14 + 0, `spec-files.md` 20 + 5, `questions.md` 0. Per-form counts: `rg -o 'cpd pr [a-z]+|gh pr [a-z]+|--pr\b|pr\.[a-z.]+|`pr-[a-z-]+`|`pr_[a-z_]+`|<pr-key>|pr-description\.md|pr add|pr edit|pr open|pr push|pr describe' <file> | sort | uniq -c`.
- Sense assignment: command forms by their verb (below); prose tokens by sentence pattern (a short list of phrases such as "adopted PR", "an open PR", "stacked PR", "before the PR opens", "PR comment", "posted PR", "repoints the next PR" marked `mixed` or `pull request`; every other prose `PR` is `object` by elimination, **unverified token by token**: the counts for `object` are an upper bound and the `mixed` / `pull request` counts a lower bound).
- Vocabulary pinned by tests (not audited, listed so a rename knows what breaks): `pi/skills/skill.test.ts` (82 PR hits; for example line 639 `never with a sync (or merge )?Job`), `pi/skills/skill-retry.test.ts:32` (`"- **A PR's open or update Job**"`), `pi/test/updates.test.ts` (24 hits: lines 372-380, 443, 473, 490-491 pin `PR api opened:`, `2 PRs opened`, `PR mainfix 2 running`), `pi/test/testguard.test.ts:91,118` (the full `φ:` sentence), `pi/test/reminder.test.ts:14,29,83`, `pi/test/delivery.test.ts:28,95,111`, `pi/test/policy.test.ts:26,30` (`## pr (owner:/p/o.toml)`).

## Terms

One row per term or word-to-watch used in more than one sense, loosely, stale, wrong or colliding. Quotes are at most 15 words.

| Term | Where (path:line) | Sense as used here | Collision | Verdict | Proposed wording |
|---|---|---|---|---|---|
| PR vs Pull Request | `pi/skills/cpd-lead/SKILL.md:8` | "each in one repo, with its own Lead Branch and Pull Request": the PR is the cpd object and has a pull request | internal: CONTEXT.md PR entity; upstream: GitHub pull request | overloaded | Workflow for the object, "pull request" for GitHub's; the sentence then reads "each Workflow ends in one pull request" |
| PR (step of Deploy) | `pi/skills/cpd-lead/SKILL.md:24`, `pi/skills/cpd-lead/SKILL.md:350`, `pi/skills/cpd-manager/SKILL.md:23` | step name for the pull request is opened (no quote: paraphrase); heading `### PR` | internal: PR object, PR phase (retired) | stale | name the step for what happens, for example "Opening" (proposal only; the User decides) |
| Plan: entity, stage, phase | `pi/skills/cpd-lead/SKILL.md:135`, `pi/skills/cpd-lead/SKILL.md:137` | "## 2. Plan" is the stage; "Phases `plan` and `dispatch`" is the phase list under it | internal: Plan (entity), Plan phase, Plan stage | overloaded | always write "Plan stage", "Plan phase `plan`", "the Plan"; one-time table of the three |
| Deploy stage vs phase `deploy` | `pi/skills/cpd-lead/SKILL.md:332` | "The end of phase `dispatch`" then "phase `deploy` until `done`": the stage starts inside another phase | internal: stage vs phase | loose | say "the Deploy stage begins when the Plan is in `dispatch` with all other Jobs merged" or realign stage and phase |
| Context (entity, stage, field, conversation) | `pi/skills/cpd-lead/SKILL.md:55`, `pi/skills/cpd-lead/SKILL.md:97`, `pi/skills/cpd-lead/references/spec-files.md:12` | Context entity; "## 1. Context" stage; Spec field `context`; "are in your context at your next turn" = the model's conversation | internal: four senses; upstream: LLM context window | overloaded | capital C only for the entity and the stage; "your conversation" for the model's; keep the JSON key `context` but call it "the Spec's `context` field" |
| Specifying | `pi/skills/cpd-lead/SKILL.md:8`, `pi/skills/cpd-lead/SKILL.md:119` | step "### Specifying" records decisions and assumptions; the Specs are written in Dividing; "Turn the User's intent into Specs" | internal: Spec (Job's Spec), Worker Spec | overloaded | name the step for what it does ("Recording"), keep Spec for the Job's Spec only |
| Spec (three things) | `pi/skills/cpd-lead/references/spec-files.md:5`, `core/src/spec/worker.md:1` | The spec file (heading) is JSON the Lead writes; "Worker Spec" is the Markdown the Worker reads; "Spec" alone is either | internal | loose | "Spec file" for the JSON, "Worker Spec" for the rendered document, "Spec" only when both are meant |
| Epic / Ticket / Task (level table) | `pi/skills/cpd-lead/SKILL.md:15`, `pi/skills/cpd-manager/SKILL.md:15` | the level table row Ticket, Plan, Lead: Epic = Context, Ticket = Plan, Task = Job | upstream: Jira Epic, ticket, task; internal: CONTEXT.md:89 `_Avoid_: ... epic` and CONTEXT.md:108 maps a Plan to a "Jira task", not a ticket | overloaded | state the table as an analogy ("Jira analogy"), and decide whether a Plan maps to a Jira task or ticket; the Task/Job row clashes with CONTEXT.md:108 |
| ticket | `pi/skills/cpd-lead/SKILL.md:164`, `pi/skills/cpd-lead/SKILL.md:103`, `pi/skills/cpd-manager/SKILL.md:78` | "one ticket per PR, never the Plan's" (Jira ticket on the Workflow); "the ticket id in lowercase" (the Plan id); "an Epic, a PRD, an ADR, a ticket" (any intake) | upstream: Jira; internal: Ticket = Plan in the table | overloaded | "Jira issue key" for the field; "Plan id" for the Plan; the table says the Plan is the ticket, line 164 says it is not |
| Epic (as intake) | `pi/skills/cpd-manager/SKILL.md:78` | "an Epic, a PRD, an ADR, a ticket": a Jira Epic the User brings | upstream: Jira Epic; internal: Epic = Context | overloaded | "a Jira Epic" when the Jira object is meant |
| brief | `pi/skills/cpd-lead/SKILL.md:30` | "write Spec files and briefs": a Lead writes briefs, but Brief is the Manager's document | internal: Brief (CONTEXT.md:204) | loose | "Spec files and outlines" or "files outside the repo" |
| Merged / merge | `pi/skills/cpd-lead/SKILL.md:41` | "A PR waits on the PRs it needs to merge": Workflow status merged = its pull request squash-merged; elsewhere a Job is Merged into the Lead Branch; `git merge origin/<base>` is a third | internal: Merged (Job), `git merge`; upstream: GitHub squash merge | overloaded | "Merged" only for Job and Workflow status; "squash-merged" for the pull request; "merge the base" for git |
| open | `pi/skills/cpd-lead/SKILL.md:352` | "Never run `cpd pr open` for a PR that is open or whose open Job is live": a command, a status and a Job kind in one sentence | internal: Open Job, status `open`, open question; upstream: GitHub open pull request | overloaded | "for a Workflow whose pull request is open or whose open Job is live" |
| draft | `pi/skills/cpd-lead/SKILL.md:342` | "ready and never a draft": a draft pull request, while a draft Job (`pi/skills/cpd-lead/SKILL.md:41`) is a Job state | internal: Draft; upstream: GitHub draft pull request | overloaded | "a draft pull request" in full |
| ready | `pi/skills/cpd-lead/SKILL.md:366`, `pi/skills/cpd-lead/SKILL.md:342`, `pi/skills/cpd-manager/SKILL.md:105` | "`ready for review` with a Post action" (Inbox row); a ready pull request (not draft); a PR is ready when its Gate passed (Manager); Ready Job | internal: Ready Job, Inbox label; upstream: GitHub "Ready for review" button | overloaded | give the Inbox label its own word ("postable"); keep "ready" for Job and for GitHub's draft state |
| Gate key `pr.gate` | `pi/skills/cpd-lead/SKILL.md:30`, `pi/skills/cpd-lead/SKILL.md:244`, `pi/skills/cpd-lead/SKILL.md:342`, `pi/skills/cpd-lead/SKILL.md:346`, `pi/skills/cpd-lead/SKILL.md:348` | "`pr.gate = local`" (7 uses): one key for all Gate runs | internal: `[gate]` has two keys, `merge` and `pr` (`core/src/policy/default.toml:5-7`); `docs/commands.md:1739` says open and push use `gate.merge`, rebase uses `gate.pr` | wrong | quote the real keys: `gate.merge` for open and push, `gate.pr` for rebase (verified against default.toml and commands.md:1739); the rename then decides what `gate.pr` is called |
| G2 / G4 / G | `pi/skills/cpd-lead/SKILL.md:29`, `pi/lib/pnguard.ts:16`, `pi/lib/askguard.ts:1`, `pi/skills/cpd-lead/references/spec-files.md:60` | "G2 (the 1500-character limit)" in the skill; `G2` is also the plannotator-link refusal; `G` is also the gemini Tier | internal: three codes named G2, one Tier G, the Gate | overloaded | name the guard ("question-length guard"), not a number |
| approve / Approving | `pi/skills/cpd-lead/SKILL.md:205`, `pi/skills/cpd-lead/SKILL.md:255` | "### Approving" (Plan review); `approved` as a `pr-feedback` reason (GitHub review); `cpd job approve-tests`; `cpd_host_approve` | internal: four approvals; upstream: GitHub "Approved" review | overloaded | prefix: "Plan approval", "review approval (GitHub)", "test-path approval" |
| review / Reviewing | `pi/skills/cpd-lead/SKILL.md:326`, `pi/skills/cpd-lead/SKILL.md:362` | "A review Job belongs to a PR like any Job"; step "Reviewing and Monitoring" is GitHub review; `cpd_plan_review` is the Plan review | internal: Review Job, Plan review, review thread; upstream: GitHub review | overloaded | "Plan review", "review Job", "pull request review" always in full |
| check / checks | `pi/skills/cpd-lead/SKILL.md:24`, `pi/skills/cpd-lead/SKILL.md:342` | step "Mechanical checks" (done by cpd: "You run none of them"); quick check; CI checks (`gh pr checks`); `check_failed` | internal: Quick check, Mechanical checks; upstream: GitHub checks | overloaded | "CI checks" for GitHub's; the step named for the Gate or opening |
| Dispatching | `pi/skills/cpd-lead/SKILL.md:242`, `pi/skills/cpd-lead/SKILL.md:32` | "### Dispatching" is a step the Lead does not do: "Never run what belongs to cpd itself" lists `cpd job dispatch`: the engine dispatches | internal: Dispatch (phase), `cpd job dispatch` | loose | "Dispatching" is what the engine does; name the Lead's step for what the Lead does (reacting to Events) |
| lock / Locked | `pi/skills/cpd-lead/SKILL.md:41`, `pi/skills/cpd-lead/SKILL.md:348` | "After the lock" (`cpd plan lock`); `lock_timeout` is a file lock to "run it again" | internal: Locked (Plan), file locks | loose | "the Plan lock" vs "a file lock" |
| poll | `pi/skills/cpd-lead/SKILL.md:40`, `pi/skills/cpd-lead/SKILL.md:248` | "Never poll, sleep or run a foreground loop" against "You may run `cpd pr poll`": poll = loop in one, one-shot refresh in the other | internal: PR poll (engine), `cpd pr poll` | loose | "refresh" for the one-shot; "poll" only for loops |
| post | `pi/skills/cpd-lead/SKILL.md:366`, `pi/skills/cpd-lead/SKILL.md:375` | "Never post with Slack tools by hand" and "A posted PR": a Slack message about a pull request; also board post | internal: PR post, board post; upstream: Slack, HTTP post | overloaded | "peer-review post (Slack)" vs "board post" |
| waiting / blocked | `pi/skills/cpd-lead/SKILL.md:228`, `pi/lib/state.ts:2` | "The other PRs stay `waiting`" (Workflow status) vs the Session states `"blocked"` and `"waiting"` | internal: Workflow status, Session state, Job `queued` waiting | overloaded | Workflow status `waiting` and Session state `waiting` need distinct names |
| base | `pi/skills/cpd-lead/SKILL.md:149`, `pi/skills/cpd-lead/SKILL.md:41` | `--base <branch>` and `--base <pr-key>`: one flag takes a branch or a Workflow key | internal | overloaded | two flags or one named value kind (the User decides) |
| needs | `pi/skills/cpd-lead/SKILL.md:188` | "cpd pr edit <key> --needs <k1,k2>": `<k>` is the Job key placeholder everywhere else, here it means PR keys | internal: Job Dependency `--needs` vs PR needs | loose | `<workflow-keys>` in placeholders |
| ask | `pi/skills/cpd-lead/SKILL.md:29`, `core/src/spec/worker.md:56` | "it records an open question in the Context and asks nobody" (`cpd context ask`); `cpd ask` asks the Lead; `ask_user_question` asks the User | internal: three asks | wrong | `cpd context ask` is described as misleading by its own text; rename or warn at the command (the User decides) |
| Escalation / Worker question | `pi/lib/review.ts:16`, `pi/skills/cpd-lead/SKILL.md:255` | label "φ: Worker question" for what the skills call an Escalation | internal | loose | one name; "Escalation" is the glossary term |
| sync Job | `pi/skills/cpd-lead/SKILL.md:385` | "never with a sync Job": a Job kind that does not exist | internal: CONTEXT.md:129 `_Avoid_: sync Job, rebase Job` | stale | "Update Job"; `pi/skills/skill.test.ts:639` pins the regex and moves with it |
| all-merged | `pi/lib/event.ts:17`, `pi/lib/delivery.ts:8`, `pi/skills/cpd-manager/SKILL.md:100` | Event kind listed although "is no longer appended"; Manager heading "Oversee all-merged PRs" | internal | stale | drop the kind if the engine no longer emits it (unverified: Rust side not read); reword the heading |
| workflow | `core/src/policy/default.toml:54` | `workflows = []` under `[deploys]`: GitHub Actions workflows of a deploy | upstream: GitHub Actions workflow; internal: new Workflow | overloaded | keep "GitHub Actions workflow" in full in text; the key `deploys.workflows` needs the User's ruling |
| deploy / deploys | `core/src/policy/default.toml:23`, `core/src/policy/default.toml:53`, `pi/skills/cpd-lead/SKILL.md:381` | domain `[deploy]` (text) and domain `[deploys]` (workflows): near-identical names, different content; also the Deploy stage and "NO DEPLOYS ON FRIDAYS" (`pi/skills/cpd-lead/SKILL.md:381`, a Danger rule) | internal: Deploy stage; upstream: deploy | loose | one name per domain; a note that a deploy rule on GitHub is not the Deploy stage |
| `pr` policy domain | `pi/lib/policy.ts:22`, `core/src/policy/default.toml:10` | the cpd-policy section header is the domain name, for example `## pr` with the source in brackets; `[pr]` holds `jira_required` (Workflow) and `[pr.feedback]`, `[pr.post]` (pull request) | internal | mixed | split by sense or rename after the decision; the header is shown to every agent |
| owner | `pi/lib/policy.ts:22`, `pi/skills/cpd-lead/SKILL.md:164` | the source label `owner:` is a policy scope; "A PR of an owner whose policy sets" is the GitHub owner | upstream: GitHub owner; internal: Known owner | loose | "the owner's policy" is fine; say "owner scope" when the file is meant |
| host | `pi/skills/cpd-manager/SKILL.md:141`, `pi/skills/cpd-lead/references/spec-files.md:30` | "The Context `host`" (a Manager) vs "declared in the host, owner or repo policy" (a machine scope) | internal: host Context, Machine policy | overloaded | "machine" for the policy scope, `host` only for the Context |
| session | `pi/skills/cpd-lead/SKILL.md:36` | "Session commands" take a Plan id (that Plan's Lead Session): cpd Session; lowercase "session" is also a pi session and a fork | internal: Session; upstream: pi session, tmux session | loose | capital S only for the cpd Session |
| message / Message | `pi/skills/cpd-lead/SKILL.md:67`, `pi/skills/cpd-manager/SKILL.md:118` | "### Communication rules": Message (`cpd session send`), Board, Record; lowercase "message" also means any Event text the adapter delivers | internal: Event, Message; upstream: pi `custom_message` | loose | "Message" only for `cpd session send`; "Event text" for the rest |
| run | `pi/skills/cpd-lead/SKILL.md:30`, `pi/skills/cpd-lead/SKILL.md:252` | `gh run view <run-id>` (GitHub Actions run), `run/<plan>/reviews/<k>.md` (directory), "the PR's open Job runs once" | upstream: GitHub Actions run; internal | overloaded | "CI run" for GitHub's |
| work | `pi/skills/cpd-lead/SKILL.md:386` | "only `work` Jobs": the Job kind `work` vs planned work and decided work (`pi/skills/cpd-lead/SKILL.md:34`, `pi/skills/cpd-lead/SKILL.md:41`) | internal: Job kind | loose | "kind `work`" when the kind is meant |
| adopted PR | `pi/skills/cpd-lead/SKILL.md:156` | "An adopted PR is added, specified, reviewed and locked like any PR": the Workflow adopts a pull request | internal / upstream | mixed | "an adopted Workflow" (a Workflow that adopted a pull request) |
| DRY: rules restated | `pi/skills/cpd-manager/SKILL.md:99`, `pi/skills/cpd-lead/SKILL.md:192` | Manager line 99 says "tell the Manager with" `cpd session send`: text copied from the Lead skill, the Manager reading it is the Manager | internal | wrong | the Manager's skill should say the Lead tells the Manager; the stacked-PR block (Manager 101-105, Lead 354-360) and the Communication rules restate each other |

Fine as used (one line): Plan (entity), Job, Job kind, Open Job, Update Job, Dependency, Attempt, Ledger, Locked, Draft (Job), Session (cpd), Event, Inbox item, Cockpit, Engine, Feedback item, User, Manager, Pending work, Context record, Context store, Lead, Worker, Escalation (as a word), Gate, Quick check, Lead Branch, Tier, Axiom, Policy, Domain, Scope, Repo policy file, Resolved policy, Machine policy, MCP server, Attach, Resource, Slot, Tenant, Skipped, Exposure, Policy key, Fallback. Words to watch with no problem found: board (a Jira board does not clash in the text read), record, decision, requirement, assumption, outline, finding, stop/stopped, fail/failed, push, event, branch, owner (see row above), archive, park, hold, queue (`queued` is only a Job status), ready (see row above for the three senses), cancel, `status` and `state` (Job and Plan `status`, Session `state`, kept distinct). `workflow`, `task`, `epic` (lower case): no hit in the Lead or Manager skill; `core/src/spec/worker.md` and `distill_prompt.md` have no PR token.

## PR occurrences

Token counts by group. Sense codes as defined; no `phase` use survives in the part (`rg -n -i 'PR phase|phase PR'` finds none: the phase is already `deploy`), no `other` use. The `step` uses are the four listed individually.

### By group

| Where (section or path:line range) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| `pi/skills/cpd-lead/SKILL.md:1-25` frontmatter and intro | 6 | object 3, pull request 1, mixed 1, step 1 (line 24) | "one deliverable, one or more PRs across one or more repos" | `object` -> Workflow; mixed and step rows below |
| `pi/skills/cpd-lead/SKILL.md:26-46` Rules | 36 | object 25, pull request 2, mixed 9 | "A PR waits on the PRs it needs to merge" | rule R1; mixed rows below |
| `pi/skills/cpd-lead/SKILL.md:47-96` Policies, Context updates, Manager, User command | 5 | object 4, mixed 1 | "`cpd policy show --pr <key>` a PR's repo policy" | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:97-138` 1. Context | 6 | object 6 | "Its repos come from its PRs." | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:139-181` Dividing | 63 | object 53, pull request 6, mixed 4 | "**A PR is a substantial deliverable.**" | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:182-204` Sequencing | 29 | object 29 | "**PR needs before the lock:**" | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:205-241` Approving | 27 | object 21, mixed 6 | "The other PRs stay `waiting`." | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:242-260` Dispatching | 58 | object 35, pull request 8, mixed 15 | "A PR of the Plan needs attention" | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:261-329` Later batch to Judging material | 42 | object 25, pull request 5, mixed 12 | "A review Job belongs to a PR like any Job" | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:330-349` Deploy intro, Mechanical checks | 39 | object 19, pull request 5, mixed 14, step 1 (line 332) | "Per PR: every step names the PR with `--pr <key>`" | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:350-361` step PR | 44 | object 26, pull request 8, mixed 9, step 1 (line 350) | "Never run `cpd pr open` for a PR that is open" | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:362-378` Reviewing, Posting | 26 | object 6, pull request 18, mixed 2 | "A PR that is ready to post for peer review" | `pull request` stays "PR" |
| `pi/skills/cpd-lead/SKILL.md:379-390` Addressing | 29 | object 17, pull request 1, mixed 11 | "On an adopted PR a moved head" | rule R1 |
| `pi/skills/cpd-lead/SKILL.md:391-399` Review comments | 9 | object 2, pull request 6, mixed 1 | "the PR has open feedback items" | `pull request` stays "PR" |
| `pi/skills/cpd-lead/SKILL.md` total | 419 | object 271, pull request 60, mixed 85, step 3 | | |
| `pi/skills/cpd-manager/SKILL.md:1-143` | 52 | object 43, pull request 5, mixed 3, step 1 (line 23) | "Within one Plan, a PR's needs" | rule R1 |
| `pi/skills/cpd-lead/references/pr-description.md:1-104` | 14 | object 5, pull request 5, mixed 4 | "naming registered Job and PR keys for follow-up work" | rule R1 for the 5; mixed rows below |
| `pi/skills/cpd-lead/references/spec-files.md:1-104` | 25 | object 22, pull request 1, mixed 2 | "run by the PR's open Job" | rule R1 |
| `pi/skills/cpd-lead/references/questions.md:1-58` | 0 | none | | |
| `core/src/spec/worker.md:1-223` | 13 | object 6, pull request 5, mixed 2 | "This is the **open Job** of a PR" | rule R1 |
| `core/src/policy/default.toml:1-54` | 4 | pull request 2, mixed 2 | `[pr.feedback]` | see rows below |
| `core/src/context/distill_prompt.md:1-28` | 0 | none | | |
| `pi/lib/*.ts` strings and Event kinds read, rows below (`updates.ts`, `reminder.ts`, `mainguard.ts`, `testguard.ts`, `event.ts`, `turns.ts`, `delivery.ts`, `noblock.ts`, `slack-post.ts`) | 31 | object 12, pull request 5, mixed 14 | `plural(e.length, "PR", "PRs")` (`pi/lib/updates.ts:206`) | see rows below |
| `pi/lib/slack-parse.ts:12` and 5 more lines (`PR_URL_RE`, `extract_prs`) | 6 | pull request 6 | `const PR_URL_RE = /github\.com` | stays "PR" |

Rule R1 (mechanical, no row each): every `object` token is rewritten to Workflow: prose "PR"/"PRs" -> "Workflow"/"Workflows", `cpd pr <verb>` -> `cpd workflow <verb>` for the verbs that act only on the object (`add`, `edit`, `cancel`, `start`), `--pr <key>` -> `--workflow <key>`, `<pr-key>` -> `<workflow-key>`, the `prs` key of `cpd plan show` -> `workflows`, Events `pr-added`, `pr-edited`, `pr-cancelled`, `pr-started`, `pr-base-changed`, error codes `pr_start_failed`, `pr_locked`. The command names are the User's decision; this row only fixes the count.

### Command forms (Lead skill, 168 tokens; Manager 10; spec-files 5)

| Where (lines of `pi/skills/cpd-lead/SKILL.md`) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| `--pr` (47), `cpd pr add` (14), `pr add` (3), `<pr-key>` (9), `cpd pr edit` (6), `pr edit` (1), `cpd pr cancel` (4), `cpd pr start` (1), `pr-started` (2), `pr_start_failed`, `pr_locked`, `prs` (3), total | 91 | object | "`cpd pr add <key> --repo <path> --title`" (line 148) | rule R1 |
| `cpd pr poll` 248, 364, 372; `cpd pr post` 366 x2, 372 x2, 377; `cpd pr reply` 397; `cpd pr merge` 377 | 10 | pull request | "`cpd pr post --pr <key>` only when the User tells you to post it" (line 366) | `pr poll`, `pr post`, `pr reply`, `pr merge` keep "pr" (GitHub's) |
| `pr-feedback` 255, 273, 348, 364, 393; `gh pr checks/view/merge/edit` 30, 156 x2, 357, 373; `pr_closed` 313, 347; `pr.post.enabled` 370 | 12 | pull request | "`pr-feedback` with reason `comments` means the PR has open feedback items" (line 393) | stays |
| `cpd pr open` 33, 55, 164, 295 x2, 348, 352, 364 (the last two `pr open` forms included) | 8 | mixed: acts on a Workflow and opens its pull request | "`cpd pr open` is the repair" (line 33) | `workflow open`? decision for the User |
| `cpd pr view` 156, 248, 255, 381, 382, 395 | 6 | mixed: shows the Workflow and its pull request's checks, reviews, comments | "`cpd pr view --pr <key>` to read a PR's failing checks" (line 248) | decision for the User |
| `cpd pr push` / `pr push` 30, 257, 295, 358, 384 x2 | 6 | mixed: pushes the Workflow's Lead Branch to its pull request | "`cpd pr push --pr <key>`" (line 384) | decision for the User |
| `cpd pr rebase` 30, 253, 257, 295, 358, 384 x3, 385 | 9 | mixed: rebases the Workflow's Lead Branch, pushes to the pull request | "`cpd pr rebase --pr <key>` rebases the unpushed commits" (line 384) | decision for the User |
| `cpd pr describe` / `pr describe` 230, 257, 313, 340, 352, 383 | 6 | mixed: writes the Workflow's text that becomes the pull request body | "`cpd pr describe --pr <key> --file <body.md>`" (line 230) | decision for the User |
| `pr.gate` 30, 244 x2, 342 x2, 346, 348 | 7 | mixed and wrong key: see Terms | "`pr.gate = local`" (line 244) | per the Gate-key row |
| `pr.jira_required` 164 | 1 | mixed: Jira ticket on the Workflow, checked at pull request open | "policy sets `pr.jira_required` needs `--jira <KEY>`" (line 164) | decision for the User |
| `pr_title_required` 313, 347, 348 | 3 | mixed: the Workflow's `--title` that becomes the squash subject | "`pr_title_required` (`cpd pr edit <key> --title`" (line 313) | decision for the User |
| `pr-description.md` 209 x2, 230 x2, 257, 313 x2, 340 x2 | 9 | mixed: the file is the Workflow's text that becomes the pull request body | "[references/pr-description.md](references/pr-description.md)" (line 209) | decision for the User |
| `pi/skills/cpd-manager/SKILL.md:104` and lines 35, 44, 99, 103: `cpd pr add` 4, `--pr` 2, `gh pr edit`, `cpd pr rebase`, `cpd pr push` (10) | 10 | object 7, pull request 1 (`gh pr edit`:103), mixed 2 (`rebase`, `push`:104) | "`cpd pr push --pr <key>`, not `cpd pr rebase`" (line 104) | rule R1 and the decisions above |
| `pi/skills/cpd-lead/references/spec-files.md:70` and lines 30, 102: `--pr` 2, `pr describe` 1, `cpd pr add` 1, `<research-pr>` 1 | 5 | object 4, mixed 1 (`pr describe`:70) | "written with pr describe `--pr <key> --file <body.md>`" (line 70) | rule R1 |

### Every `mixed`, `step` and `phase` prose use, individually

| Where (path:line) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| `pi/skills/cpd-lead/SKILL.md:3` | 1 | mixed: the open Job opens the pull request of the Workflow | "checks and opens each PR" | "opens each Workflow's pull request" |
| `pi/skills/cpd-lead/SKILL.md:8` | 1 | mixed: PR and Pull Request set side by side | "its own Lead Branch and Pull Request" | "each Workflow ends in one pull request" |
| `pi/skills/cpd-lead/SKILL.md:24` | 1 | step | "Mechanical checks, PR, Reviewing and Monitoring, Addressing" | rename the step |
| `pi/skills/cpd-lead/SKILL.md:30` | 1 | mixed: a stacked PR is a Workflow whose pull request targets a branch | "merging a base PR's new commits into a stacked PR" | "a stacked Workflow" |
| `pi/skills/cpd-lead/SKILL.md:33` | 2 | mixed: cpd opens the pull request of every started Workflow | "**cpd opens the PRs, you do not.**" | "cpd opens each Workflow's pull request" |
| `pi/skills/cpd-lead/SKILL.md:33` | 1 | mixed: "open a PR" is open the pull request | "never wait for anything to open a PR" | "open a pull request" |
| `pi/skills/cpd-lead/SKILL.md:41` | 2 | mixed: "stacked" is a Workflow property, its target a pull request base | "A stacked PR (`--base <pr-key>`) needs neither flag" | "a stacked Workflow" |
| `pi/skills/cpd-lead/SKILL.md:156` | 1 | mixed: the Workflow adopts a pull request | "An adopted PR is added, specified, reviewed and locked" | "an adopted Workflow" |
| `pi/skills/cpd-lead/SKILL.md:230` | 1 | mixed: the pull request opens, the Workflow does not | "stays available before the PR opens" | "before the pull request opens" |
| `pi/skills/cpd-lead/SKILL.md:252` | 1 | mixed | "Do not open a PR yourself" | "Do not open a pull request yourself" |
| `pi/skills/cpd-lead/SKILL.md:252` | 1 | mixed: status `open` of the Workflow = its pull request open | "A fix Job of an open PR is done" | "of a Workflow whose pull request is open" |
| `pi/skills/cpd-lead/SKILL.md:253` | 1 | mixed | "On an adopted PR, after `push_failed`" | "adopted Workflow" |
| `pi/skills/cpd-lead/SKILL.md:255` | 1 | mixed: the Event is pull request feedback on a Workflow | "A PR of the Plan needs attention" | "A Workflow of the Plan has pull request feedback" |
| `pi/skills/cpd-lead/SKILL.md:257` | 1 | mixed | "after a fix Job on an open PR run" | "on a Workflow whose pull request is open" |
| `pi/skills/cpd-lead/SKILL.md:257` | 1 | mixed | "On an adopted PR, `push_failed` of its open or update Job" | "adopted Workflow" |
| `pi/skills/cpd-lead/SKILL.md:273` | 1 | mixed | "On a PR that is already open" | "On a Workflow whose pull request is open" |
| `pi/skills/cpd-lead/SKILL.md:319` | 1 | mixed | "No local review is required before the PR opens" | "before the pull request opens" |
| `pi/skills/cpd-lead/SKILL.md:319` | 1 | mixed | "so the PR cannot open before you have read" | "so the pull request cannot open" |
| `pi/skills/cpd-lead/SKILL.md:326` | 1 | mixed | "the PR's open Job then opens the PR" | "the Workflow's open Job then opens its pull request" |
| `pi/skills/cpd-lead/SKILL.md:332` | 1 | step | "Steps: [Mechanical checks](#mechanical-checks), [PR](#pr)" | rename the step |
| `pi/skills/cpd-lead/SKILL.md:334` | 2 | mixed | "**No local review before the PR opens.** A PR is reviewed after it opens" | "pull request" for both |
| `pi/skills/cpd-lead/SKILL.md:338` | 1 | mixed | "Before the PR opens, read new records" | "Before the pull request opens" |
| `pi/skills/cpd-lead/SKILL.md:340` | 1 | mixed | "An adopted PR is exempt" | "adopted Workflow" |
| `pi/skills/cpd-lead/SKILL.md:350` | 1 | step | "### PR" | rename the step |
| `pi/skills/cpd-lead/SKILL.md:352` | 1 | mixed: three senses of open | "for a PR that is open or whose open Job is live" | "for a Workflow whose pull request is open or whose open Job is live" |
| `pi/skills/cpd-lead/SKILL.md:354` | 1 | mixed | "or the PR below it" | "the Workflow below it" for the base, "pull request" for the draft |
| `pi/skills/cpd-lead/SKILL.md:357` | 1 | mixed: Workflow open, its pull request targets a branch | "the next PR targets the previous PR's branch" | "the next Workflow's pull request targets the previous Workflow's branch" |
| `pi/skills/cpd-lead/SKILL.md:358` | 1 | mixed | "not a stacked PR taking its base PR's new commits" | "stacked Workflow" |
| `pi/skills/cpd-lead/SKILL.md:360` | 1 | mixed | "Merging the earlier branch into a PR that targets the base" | "into a Workflow whose pull request targets the base" |
| `pi/skills/cpd-lead/SKILL.md:373` | 1 | mixed | "A stacked PR's auto-merge request waits until the PR is unstacked" | "stacked Workflow's" and "the pull request is unstacked" |
| `pi/skills/cpd-lead/SKILL.md:384` | 1 | mixed | "On an adopted PR a moved head" | "adopted Workflow" |
| `pi/skills/cpd-lead/SKILL.md:386` | 1 | mixed | "Job on an open PR is refused `shape_violation`" | "on a Workflow whose pull request is open" |
| `pi/skills/cpd-manager/SKILL.md:23` | 1 | step | "**Deploy** (Mechanical checks, PR, Reviewing and Monitoring, Addressing)" | rename the step |
| `pi/skills/cpd-manager/SKILL.md:100` | 1 | mixed: cpd opens the pull request when the Workflow's last Job merges | "cpd opens a PR as soon as its last Job merges" | "opens a Workflow's pull request" |
| `pi/skills/cpd-lead/references/pr-description.md:1` | 1 | mixed: title of a file about the text that becomes the pull request body | "# PR description: the Lean template" | decision for the User |
| `pi/skills/cpd-lead/references/pr-description.md:41` | 2 | mixed: the text is read on GitHub, where a "PR cache" is a pull request name | "Caching is not implemented; see PR cache" and "follow-up: PR error-reporting" | name the pull request or the Workflow key in full |
| `pi/skills/cpd-lead/references/pr-description.md:43` | 1 | mixed | "Blocks: PR shipping" | same |
| `pi/skills/cpd-lead/references/spec-files.md:70` | 1 | mixed | "On an open PR a fix is one `work` Job" | "on a Workflow whose pull request is open" |
| `core/src/spec/worker.md:212` | 1 | mixed: the error code of the Workflow's `--title` | "`pr_title_required`, `description_required`, `jira_required`, `pr_closed`: ask the Lead." | code names are the User's decision |
| `core/src/spec/worker.md:214` | 1 | mixed: "not open" is the Workflow status or the pull request | "`illegal_transition`: the PR is not open." | "the Workflow is not open" if the status is meant |
| `core/src/policy/default.toml:7` | 1 | mixed: Gate key `pr` | `pr = "none"` | decision for the User |
| `core/src/policy/default.toml:10` | 1 | mixed: domain `pr` holds `jira_required` (Workflow) | `[pr]` | split or rename |
| `pi/lib/mainguard.ts:8` | 1 | mixed: shown to every agent that tries a write in a primary checkout | "and let a PR carry the change" | "let a Workflow's pull request carry the change" |
| `pi/lib/reminder.ts:15` | 1 | mixed: trigger phrase matching the Lead's own words | `"next pr",` | add the Workflow form; a Lead may say either |
| `pi/lib/updates.ts:206` | 1 | mixed: the `pr-opened` Event is a pull request opening for a Workflow | `${plural(e.length, "PR", "PRs")} opened` | "pull request opened" |
| `pi/lib/updates.ts:209` | 1 | mixed | `${plural(e.length, "PR", "PRs")} merged` | "pull request merged" |
| `pi/lib/updates.ts:273` | 2 | mixed: key of a Workflow, URL of its pull request | `"PRs:", ...prs.map((p) => `- PR ${p.pr} opened: ${p.url}`)` | "Workflow <key>: pull request opened <url>" |
| `pi/lib/updates.ts:275` | 2 | mixed | `[prs.length, "PR opened", "PRs opened"]` | "pull request opened" |
| `pi/lib/updates.ts:56` | 2 | mixed: Event kinds | `"pr-opened", "pr-merged"` | decision for the User |
| `pi/lib/turns.ts:32` | 1 | mixed | `"--kind", "pr-opened"` | follows the Event kind |
| `pi/lib/event.ts:22` | 3 | mixed (`pr-opened`, `pr-merged`, `pr-described`) | `"pr-opened", "pr-merged"` | follows the Event kinds |

### Adapter and core rows that are `object` or `pull request` (individually, short)

| Where (path:line) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| `pi/lib/reminder.ts:8` | 2 | object | `named no registered Job or PR. Register it now` | "Workflow" and `cpd workflow add`; pinned by `pi/test/reminder.test.ts` |
| `pi/lib/testguard.ts:7` | 1 | object | `run by the PR's open Job` | "the Workflow's open Job"; pinned by `pi/test/testguard.test.ts:91,118` |
| `pi/lib/updates.ts:236` | 1 | object | "`PR ${pr.key}`" | "Workflow ${key}"; pinned by `pi/test/updates.test.ts:490` |
| `pi/lib/updates.ts:146` | 3 | object (146, 153), object 1 + pull request 1 (147: `pull_request` field) | `a PR lacks key, status or jobs` | "a Workflow lacks key"; `pull_request` stays |
| `pi/lib/event.ts:22` | 5 | object (`pr-added`, `pr-edited`, `pr-cancelled`, `pr-started`, `pr-base-changed`) | `"pr-added", "pr-edited"` | Event kinds renamed with the object (R1) |
| `pi/lib/event.ts:17` and `pi/lib/delivery.ts:13` | 2 | pull request | `"pr-feedback": { turn: true, deliverAs: "steer" }` | stays |
| `pi/lib/noblock.ts:62` | 1 | pull request | `"gh pr checks --watch"` | stays |
| `pi/lib/slack-post.ts:1` | 1 | pull request | "The Slack send helper of `cpd pr post`" | stays |
| `core/src/spec/worker.md:16` | 3 | object 2, pull request 1 | "you bring the PR's Lead Branch up to date with its base" | R1; "pull request" stays |
| `core/src/spec/worker.md:19` | 4 | object 2, pull request 2 | "This is an **update Job** of a PR whose pull request is open" | R1 |
| `core/src/spec/worker.md:27` | 1 | pull request | "do not touch the pull request: cpd does them after your submit passes" | stays |
| `core/src/spec/worker.md:61` | 1 | object | "the Gate runs it, in the PR's open Job" | R1 |
| `core/src/spec/worker.md:212` | 1 | pull request | "`pr_closed`: ask the Lead." | stays |
| `core/src/spec/worker.md:219` | 1 | object | "the PR's policy does not resolve" | R1 |
| `core/src/policy/default.toml:14` | 1 | pull request | `[pr.feedback]` | stays |
| `core/src/policy/default.toml:17` | 1 | pull request | `[pr.post]` | stays |
| Remaining adapter hits: `pi/lib/updates.ts` doc comments and type names (about 25 lines), `delivery.ts:8`, `turns.ts:20,33`, `event.ts:7` | not classified | unverified | | comments and identifiers; not shown to an agent (unverified for `turns.ts:20`, a comment on the message a Lead gets) |

## Fix list

Only items that need a judgement; purely mechanical object -> Workflow rewrites are rule R1 above (counts per section in the table).

| path:line | What to change | Why |
|---|---|---|
| `pi/skills/cpd-lead/SKILL.md:8` | Replace "its own Lead Branch and Pull Request" | the one place that names the pull request next to the PR; says plainly the PR is not the pull request |
| `pi/skills/cpd-lead/SKILL.md:24`, `pi/skills/cpd-lead/SKILL.md:332`, `pi/skills/cpd-lead/SKILL.md:350`, `pi/skills/cpd-manager/SKILL.md:23` | Rename the step "PR" (heading, table cell, anchor `#pr`, links) | step sense; collides with the object and the retired phase |
| `pi/skills/cpd-lead/SKILL.md:3`, `pi/skills/cpd-lead/SKILL.md:33` | Reword the frontmatter description and the rule heading "cpd opens the PRs" | the open Job opens a pull request, not a Workflow; `description` is what the model sees first |
| `pi/skills/cpd-lead/SKILL.md:30`, `pi/skills/cpd-lead/SKILL.md:244`, `pi/skills/cpd-lead/SKILL.md:342`, `pi/skills/cpd-lead/SKILL.md:346`, `pi/skills/cpd-lead/SKILL.md:348` | Replace `pr.gate` by the real Gate keys (`gate.merge` for open and push, `gate.pr` for rebase) | wrong: `core/src/policy/default.toml:5-7` has `[gate]` with `merge` and `pr`; `docs/commands.md:1739` says which command reads which; the skill invents `pr.gate` |
| `pi/skills/cpd-lead/SKILL.md:156`, `pi/skills/cpd-lead/SKILL.md:253`, `pi/skills/cpd-lead/SKILL.md:257`, `pi/skills/cpd-lead/SKILL.md:340`, `pi/skills/cpd-lead/SKILL.md:384` | "adopted PR" -> "adopted Workflow" | mixed: a Workflow adopts a pull request |
| `pi/skills/cpd-lead/SKILL.md:252`, `pi/skills/cpd-lead/SKILL.md:257`, `pi/skills/cpd-lead/SKILL.md:273`, `pi/skills/cpd-lead/SKILL.md:386`, `pi/skills/cpd-lead/references/spec-files.md:70` | "open PR" -> "Workflow whose pull request is open" | mixed: status `open` and the pull request being open |
| `pi/skills/cpd-lead/SKILL.md:230`, `pi/skills/cpd-lead/SKILL.md:319`, `pi/skills/cpd-lead/SKILL.md:326`, `pi/skills/cpd-lead/SKILL.md:334`, `pi/skills/cpd-lead/SKILL.md:338` | "before the PR opens", "the PR cannot open", "opens the PR" -> "pull request" | the pull request opens; the Workflow does not (mixed) |
| `pi/skills/cpd-lead/SKILL.md:352` | Split into two sentences | three senses of open in one sentence |
| `pi/skills/cpd-lead/SKILL.md:354`, `pi/skills/cpd-lead/SKILL.md:357`, `pi/skills/cpd-lead/SKILL.md:358`, `pi/skills/cpd-lead/SKILL.md:360`, `pi/skills/cpd-lead/SKILL.md:373` | Rewrite the stacked block sentence by sentence | each sentence mixes Workflow, its branch and its pull request's base |
| `pi/skills/cpd-lead/SKILL.md:255` | "A PR of the Plan needs attention" | mixed; the Event row is the one the Lead reads most |
| `pi/skills/cpd-lead/SKILL.md:375` | "A posted PR" stays "PR" only if the User keeps "PR" for the pull request | pull request sense in Reviewing |
| `pi/skills/cpd-lead/SKILL.md:385` | "never with a sync Job" -> "never with an update Job" and `pi/skills/skill.test.ts:639` | stale: CONTEXT.md:129 `_Avoid_: sync Job, rebase Job` |
| `pi/skills/cpd-lead/SKILL.md:386-389` | Renumber the list: it runs 1-6 then restarts "3. A PR can be added after the lock", 4, 5 | wrong numbering (quote at line 387) |
| `pi/skills/cpd-lead/SKILL.md:230` | Close the placeholder "`<body.md`" | typo: unclosed `<` in "`<body.md` is a file" |
| `pi/skills/cpd-lead/SKILL.md:29` | Reword the `cpd context ask` rule | name is misleading by its own text ("asks nobody") |
| `pi/skills/cpd-lead/SKILL.md:40`, `pi/skills/cpd-lead/SKILL.md:248` | Distinguish loop polling from a one-shot refresh | "Never poll" against "You may run `cpd pr poll`" |
| `pi/skills/cpd-lead/SKILL.md:41`, `pi/skills/cpd-lead/SKILL.md:149`, `pi/skills/cpd-lead/SKILL.md:188` | `--base <branch>` vs `--base <pr-key>`; `--needs <k1,k2>` on `pr edit` | one flag, two value kinds; Job-key placeholder used for PR keys |
| `pi/skills/cpd-lead/SKILL.md:15`, `pi/skills/cpd-manager/SKILL.md:15`, `pi/skills/cpd-lead/SKILL.md:164` | Decide the Jira mapping (Plan = ticket or task) and say it once | table says Ticket = Plan; line 164 says the ticket is per PR, "never the Plan's"; CONTEXT.md:108 says Jira task |
| `pi/skills/cpd-lead/SKILL.md:30` | "Spec files and briefs" -> "Spec files and outlines" | Brief is the Manager's |
| `pi/skills/cpd-lead/SKILL.md:119` | Rename the step "Specifying" (it records, it writes no Spec) | overloaded with Spec |
| `pi/skills/cpd-lead/SKILL.md:242` | Name the step for the Lead's act | the engine dispatches; the Lead never runs `cpd job dispatch` (line 32) |
| `pi/skills/cpd-lead/SKILL.md:332` | Align stage and phase boundaries | stage Deploy starts inside phase `dispatch` |
| `pi/skills/cpd-lead/SKILL.md:366` | Rename the Inbox label "ready for review" | collides with GitHub's "Ready for review" for a draft pull request |
| `pi/skills/cpd-manager/SKILL.md:99` | Replace "tell the Manager with `cpd session send <manager-id>`" | wrong: copied from the Lead skill; the Manager is the reader |
| `pi/skills/cpd-manager/SKILL.md:100`, `pi/skills/cpd-manager/SKILL.md:101-105` | Reword "all-merged PRs"; keep one copy of the stacked-PR rules | `all-merged` is no longer an Event (`pi/lib/delivery.ts:8`); Lead 354-360 repeats the block (DRY) |
| `pi/skills/cpd-manager/SKILL.md:78` | "an Epic" -> "a Jira Epic" | Epic = Context in the table |
| `pi/skills/cpd-lead/references/pr-description.md:1`, `pi/skills/cpd-lead/references/pr-description.md:41`, `pi/skills/cpd-lead/references/pr-description.md:43` | Decide the file name and the wording for follow-up keys ("see PR cache", "Blocks: PR shipping") | text lands on GitHub, where a reader reads PR as pull request; a key is internal |
| `pi/skills/cpd-lead/references/spec-files.md:12` | Say "the Spec's `context` field" | Context overload |
| `core/src/spec/worker.md:214` | "the PR is not open" -> the Workflow status or the pull request | mixed |
| `core/src/spec/worker.md:212` | `pr_title_required`, `pr_closed`, `jira_required` | error codes the Worker reads: one is the Workflow's title, one the pull request |
| `core/src/policy/default.toml:5-10`, `core/src/policy/default.toml:14`, `core/src/policy/default.toml:17` | Decide the names of `gate.pr`, `[pr]`, `[pr.feedback]`, `[pr.post]` | `[pr]` mixes the Workflow's Jira field with pull request feedback and post |
| `core/src/policy/default.toml:23`, `core/src/policy/default.toml:53-54` | `[deploy]` vs `[deploys] workflows` | near-identical names; GitHub Actions workflow vs new Workflow |
| `pi/lib/mainguard.ts:8` | Reword "let a PR carry the change" | shown to every agent; mixed |
| `pi/lib/reminder.ts:8`, `pi/lib/reminder.ts:15` | Reword the text; add the new phrase beside "next pr" | the phrase list matches the Lead's own words; the old word stays valid for the pull request |
| `pi/lib/updates.ts:206`, `pi/lib/updates.ts:209`, `pi/lib/updates.ts:236`, `pi/lib/updates.ts:273`, `pi/lib/updates.ts:275` | Reword the turn-start lines "PRs opened", "PRs merged", "PR <key>", "PRs:" | shown to Lead and Manager each turn; mixed; pinned by `pi/test/updates.test.ts` lines 372-380, 443, 473, 490 |
| `pi/lib/event.ts:17`, `pi/lib/event.ts:22`, `pi/lib/updates.ts:56` | Event kinds per Rule R1 and the `mixed` decisions (`pr-opened`, `pr-merged`, `pr-described`) | Event kinds are CLI vocabulary |
| `pi/lib/policy.ts:22` | Follows the `[pr]` domain rename | the cpd-policy section header "## pr" is shown to every agent |
| `pi/lib/pnguard.ts:16`, `pi/lib/askguard.ts:1` | Name the guards instead of G2 | two meanings of G2 inside the adapter |
| `pi/lib/review.ts:16` | "φ: Worker question" -> Escalation | glossary term |

## Counts

| Number | Value |
|---|---|
| Files in the part | 8 text files (5 skill files, 3 core texts) read in full: 808 + 305 = 1113 lines; `pi/extension.ts` 7 lines read; 56 `pi/lib` files, 5321 lines skimmed (563 string-hit lines, 56 PR-hit lines) |
| Lines read in full | 1120 (1113 + 7) |
| Rows in `## Terms` | 42 table rows plus 1 summary line |
| Rows in `## PR occurrences` | 24 group rows, 14 command-form rows, 51 individual mixed/step rows, 17 object/pull request rows: 106 |
| Rows in `## Fix list` | 39 |
| Occurrences per sense (classified, whole part) | object 359, pull request 89, mixed 112, step 4, phase 0, other 0; total 564 |
| By file | Lead skill 419 (271 / 60 / 85 / 3); Manager skill 52 (43 / 5 / 3 / 1); `pr-description.md` 14 (5 / 5 / 4 / 0); `spec-files.md` 25 (22 / 1 / 2 / 0); `worker.md` 13 (6 / 5 / 2 / 0); `default.toml` 4 (0 / 2 / 2 / 0); adapter 37 (12 / 11 / 14 / 0) |
| Caveat | the prose `object` counts are by elimination (a sentence pattern list marks `mixed` and `pull request`), so `object` is an upper bound and `mixed` / `pull request` lower bounds; the adapter's 56 hit lines include about 25 comment and identifier lines that were not classified |
