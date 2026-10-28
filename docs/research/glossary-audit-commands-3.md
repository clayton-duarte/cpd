# Glossary audit: docs/commands.md sections 10 to 14 (session to config)

Attempt `aud-commands-3-a1`, Job `aud-commands-3`. Report only: no file other than this one was changed. Line numbers are those of `docs/commands.md` at the base commit of this worktree. `path:line` below means `docs/commands.md:line`.

## Coverage

Part: `docs/commands.md` lines 2359 to 3647 (1289 lines), from `## 10. `session`` (line 2359) to the line before `## 15. The pi adapter's protocol` (line 3648).

| Range | Section | Read |
|---|---|---|
| 2359-2915 | 10 `session` (10.1 to 10.18b, 10.17) | in full |
| 2916-3004 | 10a `board` | in full |
| 3005-3055 | 11 `events` | in full |
| 3056-3128 | 12 `watch` (engine ticks, `--ensure`, `--check`, `--mark-restart`, `--restart-tmux`) | in full |
| 3129-3481 | 13 `tui`: 13.1 `tui`, 13.2 `tui view` (views, widgets, Inbox rows, legend, StatusBar, merge action, logs), 13.3 bare `cpd` | prose in full; skimmed: the dialog-kit sizes, the ASCII drawings (about 3200-3262), the glyph and colour codes and the Pulse bucket arithmetic (about 3279-3290, 3347-3395), the machine-metric commands (3396) |
| 3482-3647 | 14 `doctor`, `config`, `tiers`, `install`, `models` | in full |

Not read: nothing in the part was skipped (every line passed through the reader; skimmed means I did not check each glyph, colour or number). Not audited, out of scope: every line outside 2359-3647 (section 15 onward belongs to another part). I did not read CONTRACT or `docs/contract.md`, so every claim below about what the contract says is unverified.

Commands behind the counts (run from the worktree root; `R` is the range `2359,3647`):

- Lines of the part: `sed -n 2359,3647p docs/commands.md | wc -l` gives 1289.
- Whole-word `PR`/`PRs`: `sed -n 2359,3647p docs/commands.md | rg -o '\bPRs?\b' | wc -l` gives 65.
- `pull request`: `sed -n 2359,3647p docs/commands.md | rg -o -i 'pull[- ]requests?' | wc -l` gives 38.
- `pull_request(s)` table names: `... | rg -o 'pull_requests?' | wc -l` gives 5.
- Kebab/identifier tokens and commands (the token list behind `## PR occurrences`): `sed -n 2359,3647p docs/commands.md | rg -n -o -e '\bPRs?\b|pull[- ]requests?|`?\bpr-[a-z]+`?|pr_[a-z_]+|prs\b|pull_requests?|cpd pr [a-z]+|`pr [a-z]+|--pr <key>|`pr`'` gives 156 tokens in all (this regex overlaps the three above; 156 is the total used in `## Counts`).
- Per section tokens, same regex on each range: 2359-2915: 1; 2916-3004: 2; 3005-3055: 0; 3056-3128: 10; 3129-3481: 138; 3482-3647: 5.
- `workflow`: `sed -n 2359,3647p docs/commands.md | rg -n -i workflow` gives 3 lines (3079, 3304, 3313).
- Engine ticks: `sed -n 2359,3647p docs/commands.md | rg -n -o 'E[0-9]+'` gives 29 hits; `E15` is at 3607, 3609 (twice), 3616, 3624 only (section 14.5), never in section 12.
- Cited lines were found with `rg -n -o -e '<phrase>' docs/commands.md` and checked by `sed -n <line>p docs/commands.md`.

Other words to watch were found by reading, not by a count. `Jira`, task, ticket and epic do not occur in the part as terms.

## Terms

| Term | Where (path:line) | Sense as used here | Collision | Verdict | Proposed wording |
|---|---|---|---|---|---|
| Deploy (column, pane title) | docs/commands.md:3179 `the **Deploy column**: the machine (hostname, CPU, GPU and memory)`; :3396 `The title of the pane is ` Deploy `` | the cockpit's right column: machine lines plus the Inbox of every Context | internal: the Deploy stage and phase `deploy`; the column holds no deploy work | loose | keep if the User wants the CPD column reading (CONTEXT.md: three columns); else "Inbox column". The User decides |
| `dispatch` (widget reference) | docs/commands.md:3396 `(reference `dispatch`)` | the name of cpd 1's cockpit widget | internal: Plan phase and stage Dispatch | stale | drop the parenthesis, or say "cpd 1's dispatch column" |
| Plan view / `plan` view | docs/commands.md:3178 `the **Plan view**: one box per PR of the chat slot's Plan` | a cockpit pane | internal: Plan (entity), Plan phase `plan`, Plan stage | loose | ok when written "Plan view"; after the rename "one box per Workflow" |
| Session | docs/commands.md:2423 `the cockpit's tmux session (`tmux.cockpit_session`) exists`; :3180 `the Session's pi file` | cpd Session; tmux session; pi session file | upstream: tmux session, pi session | loose | always qualify: "tmux session", "pi session file" (already done almost everywhere) |
| Slot | docs/commands.md:2363 `**The chat slot.**`; :3075 `the ones E3 would start if a slot were free` | chat slot (a tmux pane) and a Resource slot of the `workers` pool | internal: Slot (Resource) | overloaded | "chat slot" always with `chat`; "a free Worker slot" for the pool |
| state / status | docs/commands.md:2492 `how the pi adapter reports its agent state`; :3347 `where `<state>` is `open`, `merged`, or `closed``; :3347 `Else the PR's status (`waiting`, `dispatch`) colour 8` | Session state; pull request state; PR (Workflow) status; Job status | internal: all four; `waiting`, `open`, `merged` occur in two or more | overloaded | State = Session state, status = Job or Workflow, "pull request state" for GitHub. User decides |
| waiting | docs/commands.md:2492 `waiting` (an agent waits for another agent); :3075 `W` (**waiting**: the Ready Jobs; :3347 `Else the PR's status (`waiting`, `dispatch`) colour 8` | Session state; count of Ready Jobs; PR status | internal: three meanings; `Meaning::Waiting` (3347, 3428, 3437) a fourth (a colour) | overloaded | "ready" for the W count (the Job term is Ready Job); keep `waiting` for agent waits |
| open / merged | docs/commands.md:3347 `A merged PR's level is 0 with no needs`; :3347 `where `<state>` is `open`, `merged`, or `closed``; :2436 `### 10.4 `session open`` | PR status `merged`; pull request state `merged`; Job status `merged`; Open Job; command `session open` | internal: Open Job, `job-merged`; upstream: GitHub open/merged | overloaded | after the rename "a merged Workflow" and "a merged pull request" stay apart |
| blocked | docs/commands.md:2492 `blocked` (the User has to act); :2896 `blocked by the loop guard`; :3300 `resumes a Lead or Manager blocked by the loop guard` | Session state `blocked`; `archive_blocked`; Inbox red tier "a blocker only the User can clear" (3289) | internal: Axiom "Blocked means it needs the human" | loose | `session resume` clears loop-guard blocked and a Manager may run it (unverified who may): then that state is `waiting`, not `blocked`. Needs the User's decision |
| phase (done, archived) | docs/commands.md:2931 `a Plan in phase `done` or `archived``; :2937 `the Plan is not in phase `done` or `archived`` | refusal `plan_inactive`: Plan phase | internal: CONTEXT.md phases are grounding, plan, dispatch, deploy, done; `archived` is a flag, not a phase (contract not read: unverified) | loose | "a Plan that is `done` or archived" |
| phase list | docs/commands.md:3279 `grounding grey 15, plan magenta 13, dispatch cyan 14, deploy yellow 11`; :3428 `the phase is `deploy`, not `pr`` | the five phases; 3428 names the retired `pr` phase | internal: stage Plan vs phase `plan` | stale (3428) | delete the clause at 3428; 3279 is ok |
| tier | docs/commands.md:2426 `tier = lead_tier`; :3279 `ordered together at both levels by dot-urgency tier`; :3281 `the item's **urgency** (its tier)` | model Tier (S2, O3) and the urgency tier (red, yellow, magenta, blue, cyan, white) of dots and Inbox items | internal: Tier (model) vs urgency tier | overloaded | "urgency" for the colour ladder; keep Tier for the model |
| deploy (Inbox kind) | docs/commands.md:3281 `deploy` draws U+F15A9; :3304 `the workflow run's page, in the default browser` | an Inbox item: a GitHub deployment waiting for approval | internal: Deploy stage and phase; upstream: GitHub deployment, GitHub Actions workflow run | overloaded | "a GitHub Actions deployment" in the legend line; the kind id `deploy` is the User's decision |
| workflow | docs/commands.md:3079 `reusing the PR's workflow worktree on its Lead Branch` | cpd's old word for the Lead worktree | internal: Workflow (new name for the PR object) | stale | "Lead worktree" (glossary term) |
| workflow run | docs/commands.md:3304 `the workflow run's page`; :3313 `it is the workflow run's page` | a GitHub Actions workflow run | internal: Workflow; upstream: GitHub Actions | loose | "GitHub Actions workflow run" in full |
| base | docs/commands.md:3347 `(the base PR, a reference printed bare for this Plan's own)`; :3347 `its base; then` | `base` = base branch; "base PR" = the PR this one is based on | internal: base branch vs base PR; upstream: git base branch | overloaded | after the rename "base Workflow" next to "base branch": say "base branch" for the git word |
| needs | docs/commands.md:3347 `needs <plan>/<key>` | PR needs of another Plan | internal: Job `needs`, Dependency | ok | no change |
| ready | docs/commands.md:3075 `the Ready Jobs, §3.2`; :3347 `only when the PR is ready to merge`; :3405 `**Post** (status `ready for review`)` | Ready Job; pull request ready to merge; item label `ready for review` | internal: Ready Job vs `pr-ready` | loose | keep Ready for Jobs only |
| merge (popup, view, action) | docs/commands.md:3199 `Buttons and keys are the `merge` popup's`; :3419 `what the `merge` popup's confirm starts`; :3430 `there is no confirm popup and no merge icon` | a confirm popup that no longer exists; the background action | upstream: GitHub merge | stale | delete the popup wording at 3199, 3419, 3424, and the `view merge` synopsis at 3164 if the view is gone (unverified) |
| post | docs/commands.md:2920 `### 10a.1 `board post``; :3405 `the cockpit waits for the Slack call` | `board post` writes the board; `pr post` sends the pull request to Slack | internal: PR post vs board post; upstream: Slack | loose | none for `board`; keep `pr post` as the Glossary's PR post |
| message | docs/commands.md:2918 `The message board`; :2543 `The first message is the file `plan new` or `session fork` wrote`; :2689 `Event** `session-message`` | board post; first message; `session-message` Event | internal: Event vs board post; upstream: pi custom message | loose | "board post", "first message", "message Event" |
| board | docs/commands.md:2918 `The message board: a SQLite table whose posts name who they are for` | a cpd table of posts | upstream: Jira board, GitHub project board | loose | keep "message board" in full, never "board" alone |
| review | docs/commands.md:3313 `the url is the Review page (plannotator's`; :3304 `the Session's review page` | plannotator plan review; Review Job; GitHub review (`review-requested`) | internal: Review Job, Reviewing step; upstream: GitHub review | overloaded | "plan review page" for plannotator |
| check / checks | docs/commands.md:3098 `**With `--check`** (hs-d463`; :3504 `Checks, in order:` | `watch --check`; `doctor` checks | internal: Mechanical checks, Quick check; upstream: CI checks | loose | "preflight" for `watch --check`; "doctor checks" |
| lock | docs/commands.md:3279 `a draft appended after the lock included`; :3072 `locks/watch.lock` | Plan lock (Locked); file lock | internal: Locked vs `flock` | loose | "Plan lock" vs "lock file" |
| note | docs/commands.md:3346 `no Plan yet`, `loading… (first fetch)`, or the note`; :3188 `(no note, §17 "No notes")` | a UI message line | internal: Context record kind `note` | loose | "notice line" or "message row" |
| engine ticks | docs/commands.md:3066 `then E1–E5 and E7–E14 (E6 is removed`; :3609 E15 | the tick list of `watch` | internal: Engine | stale | add E15 (models) at 3066 and 3074, or say why it is absent; unverified whether E15 is a tick of the same loop |
| owner | docs/commands.md:3341 `owner selection (label `Owner:`, options the policy files' stems sorted`; :3419 `cpd pr merge <owner>/<repo>#<n>` | policy owner vs GitHub repository owner | internal: Known owner; upstream: GitHub owner | loose | "policy owner" and "repo owner" |

Summary line for the terms that are fine: Plan, Context, Context record, Lead, Manager, Worker, Escalation, Event, Inbox, Inbox item, Cockpit, Engine, Attempt, Job, Job kind, Open Job, Update Job, Lead Branch, Tier (as model), User, Ledger, Tenant, Policy, Dispatch, Dispatching, Merged (Job), Draft, Locked, Session (cpd), `tiers`, `install`, `models`, `config` and `doctor` are used in one sense, or only in the senses listed above; branch, queue, hold, park, archive, cancel, stop, fail, push, poll, record, decision and outline are used in their ordinary or cpd sense without collision here.

## PR occurrences

Counts are tokens of the regex in `## Coverage` (156 in all), classified one by one by reading each place. Sense codes are the six of the Spec; `phase` and `mixed` rows are listed one by one. After the rename `object` becomes Workflow and `pull request` stays. Where a quote has a backtick, it is part of the text.

Groups by section (counts and lines; `xN` is N tokens on that line):

| Where | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| 10 `session`: 2693x1 | 1 | pull request | `"unblocks": "PR #210 opened"` | keep (example) |
| 10a `board`: 2973x1, 3000x1 | 2 | pull request | `"text": "PR #207 opened"` | keep (example) |
| 11 `events` | 0 | none | none | none |
| 12 `watch`, E3 and E7 and `pr_start_failed`: 3066x1, 3074x2, 3079x5 | 8 | object | `E7, the PR start, since R34` | "E7, the Workflow start"; `pr_start_failed` (3074) is an identifier: renamed with the Event rule |
| 12 `watch`, E5 and `pr_poll_seconds`: 3068x2 | 2 | pull request | `E5 (the PR poll) runs at its due time` | keep |
| 13 `tui`, Inbox kinds, pull request words, `cpd pr merge/poll/post`, `pull_requests`: 3179x1, 3281x1, 3289x3, 3301x2, 3302x6, 3303x1, 3313x3, 3346x1, 3347x12, 3367x2, 3376x1, 3401x8, 3405x6, 3407x2, 3411x1, 3419x2, 3421x6, 3428x7, 3430x1, 3431x2, 3433x2, 3435x2 | 72 | pull request | `the GitHub pull request page` | keep |
| 13 `tui`, Plan view boxes, `--pr <key>`, `prs`, `pr add`, Event kinds `pr-added/-edited/-cancelled/-started`, `pr_start_failed`, history notes: 3178x1, 3341x2, 3345x1, 3346x3, 3347x20, 3360x5, 3367x4, 3405x2, 3411x1, 3417x1, 3428x1, 3437x1, 3457x1, 3473x1 | 44 | object | `The PR boxes start at the top of the pane` | "Workflow boxes"; `--workflow <key>`; table `workflows`; `cpd workflow add` |
| 13 `tui`, `mixed`: see the rows below | 21 | mixed | see below | see below |
| 13 `tui`, `phase`: 3428x1 | 1 | phase | `the phase is `deploy`, not `pr`` | delete the clause |
| 14 `doctor`...`models`, `gh` subcommands: 3499x3 | 3 | pull request | `` `pr create`, `pr list --json` `` | keep (gh subcommands) |
| 14, history: 3565x1 | 1 | object | `Plan bifrost, PR catfp` | history, see Fix list |
| 14, `mixed`: 3512x1 | 1 | mixed | see below | see below |

Every engine tick, Event, Inbox kind, widget heading and label that carries a PR:

| Where | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| Engine tick E3 (3079) | 5 | object | `reusing the PR's workflow worktree on its Lead Branch` | "Workflow's Lead worktree" |
| Engine tick E5, the PR poll (3068) | 2 | pull request | `E5 (the PR poll) runs at its due time` | keep |
| Engine tick E7, the PR start (3066, 3074) | 3 | object | `a PR that E7 fails to start gets its own` | "Workflow start" |
| Engine tick E8, Inbox (3066) | 0 | none | none | none |
| Event `pr-feedback` (3347, 3367) | 2 | pull request | `only becoming `conflicting` appends `pr-feedback`` | keep |
| Events `pr-added`, `pr-edited`, `pr-cancelled`, `pr-started` (3367) | 4 | object | `pr-added, pr-edited, pr-cancelled, pr-started` | `workflow-*`, the User decides |
| Event `pr-merged` (3367, 3376) | 2 | pull request | `` `job-merged`, `pr-merged` `` | keep |
| Event `pr-opened` (3367, 3374) | 2 | mixed | `` `pr-opened` `` | see mixed rows |
| Inbox kinds `pr-ready` (3179, 3281, 3289, 3302, 3313, 3401, 3405, 3407, 3411, 3428, 3430) | 13 | pull request | `` a left click on a `pr-ready` row of the Inbox `` | keep |
| Inbox kinds `pr-failing`, `pr-changes` (3301) | 2 | pull request | `` `pr-failing`, `pr-changes` (history rows only `` | keep |
| Widget heading `PRs` (3193, removed section) | 1 | mixed | `──PRs───────────3` | see mixed rows |
| Widget heading ` Plan ` and label `no PR yet: cpd pr add` (3360) | 2 | object | `shows the message `no PR yet: cpd pr add`` | "no Workflow yet: cpd workflow add" |
| Widget label `PR opened` (3374, 3379, 3428) | 3 | mixed | `PR opened` | see mixed rows |

`mixed` and `phase` rows, one by one:

| Where | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| 3193 | 1 | mixed: the heading of a section that no longer exists (3407); cannot tell if it listed Workflows or pull requests | `──PRs───────────3` | drop the example |
| 3279 | 1 | mixed: a tree row has no PR status in this text | `the counters, the PR status) keeps its colours` | drop the words (unverified) |
| 3347 | 1 | mixed: a pull request state drawn on a PR box | `in the palette's PR state colours` | "pull request state colours" |
| 3347 | 1 | mixed: the pull request is ready, the box is a PR | `only when the PR is ready to merge` | "when its pull request is ready to merge" |
| 3347 | 1 | mixed: a function name of unknown meaning | `reference `pr_state`` | unverified: ask the code |
| 3347 | 1 | mixed: names the removed column; stale | `§13.2's PRs column and the legend refer to it` | "the Inbox and the legend" |
| 3367 | 1 | mixed: Event for a pull request opened for a PR | `pr-opened, pr-merged, escalation` | User decides |
| 3374 | 2 | mixed: the kind label and its Event | `PR opened` | User decides |
| 3379 | 1 | mixed: the kind label | `error, plan / ask, PR opened, escalation, merged, other` | as 3374 |
| 3396 | 7 | mixed: the removed section's rows, lines, cues, menu | `the PRs section is removed: no `PRs` title, no two-line PR rows` | delete the clause |
| 3407 | 1 | mixed: the section name | `**The PRs section** was removed by pro-d130` | delete the paragraph |
| 3411 | 1 | mixed: the PR box shows its pull request's state | `the Plan view's PR box title draws the PR's state` | "pull request's state" |
| 3428 | 1 | mixed: the legend lists pull request states of PR boxes | `Its PR states list is `open`, `merged`, `closed`` | "pull request states" |
| 3428 | 1 | mixed: the kind label | `cyan `a PR opened`, cyan `an escalation`` | as 3374 |
| 3512 | 1 | mixed: the noun `pr` covers `pr add` (no gh) and `pr merge` (gh) | `needed only by `pr` commands; verified on 2026-10-14` | name the commands |
| 3428 | 1 | phase: names the retired `pr` phase | `the phase is `deploy`, not `pr`` | delete the clause |

## Fix list

| path:line | what to change | why |
|---|---|---|
| docs/commands.md:3066, 3074, 3079 | rule: rewrite `PR` (object) as Workflow in E3 and E7 prose (8 uses); `pr_start_failed` follows the Event/code rule | `object`, mechanical |
| docs/commands.md:3079 | `the PR's workflow worktree` becomes "the Workflow's Lead worktree" | `workflow` word in cpd's old sense collides with the new Workflow; glossary term is Lead worktree |
| docs/commands.md:3178, 3341, 3345, 3346, 3347, 3360, 3411, 3417, 3428 | rule: `PR box`, `per PR`, `PR key`, `PR's Jobs`, `base PR`, `no PR yet: cpd pr add` become Workflow; about 41 uses, no row each | `object`, mechanical |
| docs/commands.md:3347 | write "pull request state" where the box shows the GitHub state; keep "Workflow status" for `waiting`, `dispatch`, `cancelled`, `merged` | `mixed`: `PR state colours`, `PR is ready to merge`, `pr_state`: three uses name both |
| docs/commands.md:3347 | replace `§13.2's PRs column` with the Inbox | stale: the PRs column was removed by pro-d130 (3407) |
| docs/commands.md:3193 | replace the example ` ──PRs───3 ` with a section that exists | stale: the `PRs` section no longer exists (3396, 3407) |
| docs/commands.md:3279 | delete `the PR status` from the tree row | stale: tree rows have no PR status (unverified; the Inbox text at 3396 says the PRs section is gone) |
| docs/commands.md:3396, 3407 | delete or shorten the removed-section text | `mixed`; prose about what is gone is history |
| docs/commands.md:3367, 3374, 3379, 3428 | decide a name for Event `pr-opened` and the kind label `PR opened` (Workflow opened it, the pull request is what opens) | `mixed`: the User decides |
| docs/commands.md:3411 | "the Workflow box title draws the pull request's state" | `mixed` |
| docs/commands.md:3428 | delete `the phase is `deploy`, not `pr`` | `phase`: `pr` was retired as a phase; CONTEXT.md says the phase is `deploy` |
| docs/commands.md:3428 | the legend entry `a deploy waits for your approval` and `` `deploy` ``: write "a GitHub Actions deployment" | `deploy` collides with the Deploy stage and phase |
| docs/commands.md:3512 | `needed only by `pr` commands` becomes "needed only by the pull request commands (`pr merge`, `pr poll`, `pr post`, ...)" | `mixed`: `pr add` needs no gh (unverified) |
| docs/commands.md:3457, 3473, 3565 | keep `PR `bare``, `PR `tmux``, `PR catfp` as history, or write "Workflow" | `object` in a provenance note; the User decides whether history here keeps old words |
| docs/commands.md:3314 | `session send <manager> --wake` | wrong: `session send` has `--unblocks`, no `--wake` (10.13 at 2659-2662: `cpd session send <id> (<text> | --file <f>) --unblocks <reason>`); the text also says `--wake` wakes the Manager, but every message already wakes (10.13) |
| docs/commands.md:3075 vs 3417 | make one definition of `N` and `W` | wrong: 3075 says N is the pool's tenant Jobs, `failed` and retried `queued` included; 3417 says N is "Attempts not `ended`"; 3075 says W is the Ready Jobs, 3417 adds "whose PR has started" |
| docs/commands.md:3199, 3419, 3424, 3430, 3164 | decide whether the `merge` popup or view exists | stale: 3430 says `there is no confirm popup`; 3199, 3419 and 3424 still describe `the `merge` popup's confirm`; 3164 still lists `tui view merge` |
| docs/commands.md:2529 | `§10.15` becomes `§10.16` | wrong: `session retier` is 10.16 (2765); 10.15 is `session first-message` (2736) |
| docs/commands.md:3064-3066, 3074 | add E15 (models), or say it runs elsewhere | stale: E15 is in 14.5 (3607) but `watch` lists E1 to E14 only |
| docs/commands.md:3075 | `if a slot were free` becomes "if a Worker slot were free" | `Slot` is the Resource term; "chat slot" is the other use |
| docs/commands.md:2682, 2931, 2937 | `phase `done` or `archived`` becomes "`done` or archived" | `archived` is not a phase (contract not read: unverified) |
| docs/commands.md:3304, 3313 | `workflow run's page` becomes "GitHub Actions workflow run's page" | upstream GitHub Actions workflow must be written in full |
| docs/commands.md:3313 | `Review page (plannotator's` becomes "plan review page (plannotator's" | `review` has four meanings in the part |
| docs/commands.md:2896, 3300 | check whether a Manager can clear a loop-guard block | Axiom: blocked means only the User can clear it; unverified who can run `session resume` |
| docs/commands.md:3098 | name `watch --check` "preflight" or keep and note | `check` is also Mechanical checks, doctor checks |
| docs/commands.md:3179, 3396 | decide: keep `Deploy column` (CPD's three columns) or rename | loose: the column holds machine and Inbox, not deploy work; the User decides |

## Counts

| Item | Number |
|---|---|
| Files | 1 (`docs/commands.md`); the findings file is the 2nd |
| Lines of the part | 1289 (2359 to 3647): all read, about 160 of them skimmed (see Coverage) |
| Rows in `## Terms` | 29, plus one summary line |
| Rows in `## PR occurrences` | 14 (sections) + 13 (ticks, Events, Inbox kinds, labels) + 16 (`mixed`/`phase`) = 43 |
| Rows in `## Fix list` | 27 |
| `object` | 53 |
| `pull request` | 80 |
| `phase` | 1 |
| `step` | 0 |
| `mixed` | 22 |
| `other` | 0 |
| Total (53 + 80 + 1 + 0 + 22 + 0) | 156 tokens of the Coverage regex |
| By section: 10 session | 1 (1 pull request) |
| 10a board | 2 (2 pull request) |
| 11 events | 0 |
| 12 watch | 10 (8 object, 2 pull request) |
| 13 tui | 138 (44 object, 72 pull request, 21 mixed, 1 phase) |
| 14 doctor to models | 5 (3 pull request, 1 object, 1 mixed) |
