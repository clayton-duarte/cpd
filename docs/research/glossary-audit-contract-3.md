# Glossary audit: contract sections 5 and 6 (events, tmux host, cockpit)

Audit of `docs/contract.md` for the Workflow rename (Context records pro-d136 and pro-d138): where `PR` means the cpd object (`object`), where it means the GitHub pull request, and where other glossary terms collide. Report only: no file other than this one changed. Part: Job `aud-contract-3`, Plan `glassary`, Attempt `aud-contract-3-a1`.

## Coverage

**Part.** `docs/contract.md:2420-3490`: from the heading `## 5. Events` (line 2420) to the line before `## 7. git and the Gate` (line 3491). The Spec's "about lines 2395 to 3465" is 25 lines early in this checkout; the part is the heading range, 1071 lines. Files: one, `docs/contract.md`. `CONTEXT.md` (lines 1-85 and the entry list) was read as the glossary, not audited.

| Range | Heading | Read |
|---|---|---|
| 2420-2427 | 5. Events, 5.1 Rules | in full |
| 2428-2503 | 5.2 Kinds (the Event table) | in full, every row |
| 2504-2552 | 5.3 Delivery by the pi adapter | in full |
| 2553-2564 | 5.4 `cpd events follow` | in full |
| 2565-2692 | 6. The tmux host, 6.1 The server (options, pane border, StatusBar, hooks, tmux 3.6 restart) | in full |
| 2693-2964 | 6.1.1 The Dango palette (colour rules, element tables, exact formats) | in full; the colour-by-element tables (2773-2960) were scanned for terms and labels, not checked cell by cell for colour |
| 2965-3099 | The pi theme, SGR tables, pi chat rows, closing rows, failed row, working label | in full; the 56-token theme table (about 2990-3030) scanned only (no cpd terms in it) |
| 3100-3143 | The input widget (dialog kit, hs-d575) | in full; the ASCII examples scanned |
| 3144-3215 | 6.2 Sessions, windows and panes; 6.3 tmux commands; 6.4 Where everything runs; 6.5 Priority | in full |
| 3216-3257 | 6.6 How cpd starts pi | in full |
| 3258-3321 | 6.6 The engine's place; 6.7 `session fork`; 6.8 Forks | in full |
| 3322-3400 | 6.9 compact-tools; the pi input widget (3369) | in full |
| 3401-3467 | Chat balloons | in full; the geometry examples scanned |
| 3468-3490 | 6.10 ask_user_question; 6.11 No MCP autoload | in full |

**Not read:** nothing in the range is unread. Not audited (outside the part): sections 1-4 and 7-10 of the contract, `commands.md`, `docs/adr`, `docs/archive`, `docs/research`.

**Method.** I read the range in chunks with the `read` tool (`offset`/`limit`), then counted with `rg` and a small script. Every Quote below was checked by script to be a literal substring of the cited line, at most 15 words. PR tokens were found with this one pattern (case-sensitive for `PR`, `PRs`; the lower-case identifiers `pr-*`, `pr_*`, `pr.*`, `--pr`, `until-pr`, `prs`, `pr`, `*_pr` and the words `pull request(s)`/`pull_requests` are in the same pattern):

```
\bPRs?\b|\bpr[-_ ][a-z_]+|\bpr\.[a-z_.]+|--pr\b|until-pr|until_pr|\bprs\b|\bpr\b|[a-z]_pr\b|pull_requests?|[Pp]ull requests?
```

The commands behind each count (run on `docs/contract.md`, range cut with `sed -n 2420,3490p`):

| Count | Command | Result |
|---|---|---|
| lines in the part | `sed -n 2420,3490p docs/contract.md \| wc -l` | 1071 |
| every PR token (pattern above) | `rg -o -n '<pattern>' docs/contract.md`, kept for lines 2420-3490 | 239 on 88 lines |
| `PR` / `PRs` only | `rg -o -n '\bPRs?\b' docs/contract.md`, kept for lines 2420-3490 | 118 on 72 lines |
| `pull request(s)` and `pull_requests` | `rg -o -n -i 'pull requests?\|pull_requests' docs/contract.md`, kept for the part | 17 |
| `workflow` / `Workflow` | `sed -n 2420,3490p docs/contract.md \| rg -o -w -i 'workflows?'` | 3 (2443, 2543, 3260) |
| headings in the part | `grep -n '^##\+ ' docs/contract.md`, kept for the part | 23 |

The pattern counts a mention of `pr-opened` as one token, and each of `from_pr`, `to_pr`, `base_pr` and `until_pr` as one. A `PR` inside a longer word (for example `PR-opened`) is one token.


## Terms

One row per term or word that is used in more than one sense, loosely, stale, wrong or colliding. Quotes are at most 15 words and sit on the cited lines.

| Term | Where (path:line) | Sense as used here | Collision | Verdict | Proposed wording |
|---|---|---|---|---|---|
| waiting | docs/contract.md:2469; docs/contract.md:2687; docs/contract.md:2833 | three meanings: Session state (open Escalation); the status line's `W waiting` (Ready Jobs); the old PR status `waiting` (before the lock) Quotes: "`waiting` since hs-d303"; "**waiting**) is the number of Ready Jobs"; "(`waiting`, `dispatch`)" | internal (Session state, Workflow status, Ready Jobs count); upstream none | overloaded | keep `waiting` for the Session state only; rename the count `ready` (`workers 4/6 · 0 ready · 6 queued`); the Workflow status is the Lead's rename to decide |
| Ready / ready | docs/contract.md:2687; docs/contract.md:2737; docs/contract.md:2460 | Ready Job (Dependencies merged); `ready to merge` (`merge_state` clean, pull request); `stays ready` (a PR kept its Gate result) Quotes: "number of Ready Jobs"; "ready to merge"; "stays ready" | internal | overloaded | `Ready Job` stays; write `mergeable` for the pull request fact; replace `stays ready` with `keeps its Gate result` |
| draft | docs/contract.md:2857; docs/contract.md:2444 | Job status `draft` (Draft) and the Deploy column's `draft` pull request (GitHub draft) Quotes: "`draft`, owned / not owned"; "`status` is `draft` for a draft added" | internal (Draft Job) and upstream (GitHub draft pull request) | overloaded | write `draft pull request` where GitHub's draft is meant; `Draft` (capital, Job) otherwise |
| open / opened | docs/contract.md:2444; docs/contract.md:2456; docs/contract.md:2496 | Job kind `open` (Open Job); PR status `open`; GitHub pull request state `open`; `review open`; `session open` (bring into the chat slot) Quotes: "engine E4 (an `open` Job)"; "its state became `closed` (from `open`)"; "`cpd review open`" | internal (4 senses) and upstream (GitHub state) | overloaded | `open Job` always with `Job`; `open` as a Workflow status becomes the Workflow rename's call; GitHub state written `pull request state open` |
| merged / merge | docs/contract.md:2446; docs/contract.md:2464; docs/contract.md:2736; docs/contract.md:2808 | Job merged into the Lead Branch (`job-merged`); the pull request squash-merged on GitHub (`pr-merged`, merge popup); status `merged` of the PR; `merging` Attempt Quotes: "\| `job-merged` \| Job → `merged`"; "`pr poll` (`open → merged`"; "merged Jobs and PRs"; "(an item's PR merge runs)" | internal; upstream (git merge, GitHub merge) | overloaded | `job-merged` keeps `merged`; the GitHub act is `pull request merged` (kind `pull-request-merged`?, User decides); Glossary `Merged` already covers both: split it |
| checks / checking | docs/contract.md:2452; docs/contract.md:2456; docs/contract.md:2743 | Mechanical checks of an Attempt's submit (`checking`, check span) vs the pull request's CI checks (`checks`, `pr.feedback.checks`) Quotes: "`check` `start` or `end` and `what` `checks`"; "its checks became `failed`"; "is shown as `checking`" | internal; upstream (GitHub checks) | overloaded | `checks` alone = pull request CI checks; Mechanical checks write `submit checks`; the derived `checking` becomes `submit-checking` or stays with a legend line |
| review / reviewing | docs/contract.md:2441; docs/contract.md:2456; docs/contract.md:2496; docs/contract.md:2807 | Plan review (User approves the Plan); pull request review (GitHub, `review_decision`); plannotator review (`review-opened`); team review requested on Slack (`review-requested`); Review Job Quotes: "\| `plan-review` \| a Plan review is requested"; "its review became `changes_requested`"; "`plannotator_reviews` row"; "`review-requested`" | internal (4); upstream (GitHub review) | overloaded | `Plan review`, `pull request review`, `plannotator review`, `team review`: never bare `review` |
| tier / Tier | docs/contract.md:2735; docs/contract.md:2807; docs/contract.md:2468 | Tier = model tier of a Session or Job (`O3`, `S2`); Inbox tier = a colour rank of an Inbox item (red, yellow, magenta, cyan, white) Quotes: "the Inbox's red tier"; "in its tier's colour"; "`{role, name, forked_from, tier}`" | internal | overloaded | call the Inbox's rank `urgency` (the Event already has `urgency`, 2486) and the colour `urgency colour` |
| Session / session | docs/contract.md:3144; docs/contract.md:3146; docs/contract.md:3156 | the heading says `Sessions` but the table is about tmux sessions (`cpd`, `cpd-work`); `Session` elsewhere is the cpd Session Quotes: "### 6.2 Sessions, windows and panes"; "\| tmux session \| Window \| Panes \| What runs \|"; "`tmux -L cpd attach -t cpd`" | upstream (tmux session) and internal (Session) | loose | heading `6.2 tmux sessions, windows and panes`; always `tmux session` for tmux's |
| Deploy | docs/contract.md:2613; docs/contract.md:2807; docs/contract.md:2440 | cockpit column and pane title `Deploy` (it lists pull requests of every owner, not the Deploy stage's work); Plan phase `deploy`; Inbox kind `deploy`; CPD stage Deploy Quotes: "\| `@cpd_view = widgets` \| `Deploy` \|"; "the Inbox row of kind `deploy`"; "`dispatch → deploy`" | internal | overloaded | keep the stage/phase; say `the Deploy column` for the pane; Inbox kind `deploy` needs its own definition (unverified: its meaning is not in this part) |
| Context / Plan (pane titles) | docs/contract.md:2611; docs/contract.md:2612 | pane titles for the Context tree and the Plan view; they match the three CPD stages (past, present, future) but the panes show entities Quotes: "\| `@cpd_view = tree` \| `Context` \|"; "\| `@cpd_view = plan` \| `Plan` \|" | internal (entity vs stage) | ok | none; one line in CONTEXT.md saying the three panes are named for the stages would remove the doubt |
| phase | docs/contract.md:2781; docs/contract.md:3080; docs/contract.md:3379 | Plan phase (grounding, plan, dispatch, deploy, done) vs the moon loader phases and the cursor's blink phase Quotes: "working Lead's dot, plan phase"; "the 28 moon_alt phases"; "visible phase and an off phase" | internal (stage, step); upstream none | loose | `loader frame` and `blink on/off` for the animation words |
| Plan phase `pr` (stale) | docs/contract.md:2687 | the retired phase `pr`; the phase is `deploy` (CONTEXT.md:15, the Event table at 2440) Quotes: "Plan in `dispatch` or `pr`" | internal | stale | `Plan in `dispatch` or `deploy`` |
| Workflow | docs/contract.md:3260; docs/contract.md:2443; docs/contract.md:2543 | cpd 1's name for Plan (retired, D3); after the rename `Workflow` is the new object, so a bare `Workflow` here reads wrongly Quotes: "one engine instead of one per Workflow"; "cpd 1's `workflow-done`"; "`workflow-done` → `plan-done`" | internal (new Workflow); upstream (GitHub Actions workflow) | stale | 3260: `one per Plan`; 2443 and 2543 name a retired cpd 1 kind: keep, but say `cpd 1's name for Plan` |
| status / state | docs/contract.md:2469; docs/contract.md:2456; docs/contract.md:2687 | Session `state`; pull request `state` and `merge_state`; Job, Attempt and PR `status` Quotes: "a Session's state or label changes"; "`state`, `checks`, `review_decision`, `mergeable` and `merge_state`"; "`queued`, every Dependency `merged`" | internal; upstream (GitHub state, mergeStateStatus) | loose | `state` for Session only; fields of the pull request keep GitHub's names but are introduced as `pull request state` |
| label | docs/contract.md:2469; docs/contract.md:3149; docs/contract.md:3105 | Session state label (`φ: Worker question`); tmux window name part; the input box label Quotes: "`label` is the label of `to`"; "`park-<label>`"; "the label as a pill" | internal; upstream (tmux window name) | loose | `reason` for the state label (the table at 2519 already says `blocking reason`); `window name` for `park-<label>` |
| owner / owned | docs/contract.md:2476; docs/contract.md:2456; docs/contract.md:2853 | Context owner (policy); a Plan that owns a pull request; repo owner in `<owner>/<repo>` Quotes: "sets or changes the owner of a Context"; "a pull request no Plan owns"; "owned yes; not owned no (right click only)" | internal; upstream (GitHub repo owner) | overloaded | `Context owner`; for pull requests `adopted by Plan` or `tracked by Plan` (User decides); `<org>/<repo>` is GitHub's `owner`, leave |
| step | docs/contract.md:2485; docs/contract.md:2460; docs/contract.md:2495 | engine step (E7's start steps), numbered steps of a poll, vs the CPD flow's `step` (Grounding, Clarifying …) Quotes: "a step's failure"; "steps 7–8"; "`details: {pr, step, tail?, log?}`" | internal | loose | `engine step` and `poll step`; reserve `step` for CPD flow steps |
| stage / stages | docs/contract.md:3084 | lifecycle moments of a tool row; CPD uses `stage` for Context, Plan, Deploy Quotes: "A group head follows the same stages." | internal | loose | `moments` or `states` of the row |
| queue / queued | docs/contract.md:2687; docs/contract.md:3243 | Job status `queued`; pi's follow-up and steering queues (`followUpMode`) Quotes: "`Q` (**queued**) is the number of Jobs with status `queued`"; "**Queue modes are the User's.**" | upstream (pi queue modes); internal | loose | `pi message queue` when pi's is meant |
| lock / locked | docs/contract.md:2440; docs/contract.md:3189; docs/contract.md:3270 | Plan lock (`plan lock`, `Locked`); file locks under `locks/` (flock); the store's setup lock Quotes: "also moves every draft Job to `queued`"; "`locks/watch.lock`"; "`locks/watch-ensure.lock`" | internal; upstream (flock) | loose | `file lock` for `locks/*.lock` |
| parked / asleep / stopped / archived | docs/contract.md:2470; docs/contract.md:2481; docs/contract.md:2469 | four ways a Session is out of view: parked (window), asleep (E13), stopped (state), archived; `parked` and `asleep` are not glossary terms Quotes: "is created, moved into the chat slot, parked or gone"; "is asleep and out of view"; "`working`, `waiting`, `blocked`, `idle`, `stopped`" | internal | loose | add `Parked` and `Asleep` to CONTEXT.md, or reuse `stopped` and `archived` |
| attach / attached | docs/contract.md:2474; docs/contract.md:3156 | Attach = MCP server attach (glossary); tmux `attach` Quotes: "`mcp attach` (commands.md §24.1)"; "The User attaches with `tmux -L cpd attach -t cpd`" | upstream (tmux attach); internal | loose | `attach the cockpit` / `tmux attach` vs `mcp attach`; glossary entry `Attach` should say MCP only |
| message | docs/contract.md:2485 | Event `text` delivered as a pi message; `session-message` (a message from a Session or the User); tmux's message line (`display-message`) Quotes: "one message to a Lead or a Manager" | upstream (pi message, tmux message) | loose | `Event message` vs `Session message`; keep `session-message` |
| brief | docs/contract.md:2508 | `brief` is a record kind; CONTEXT.md's `Brief` is the Manager's brief to a Lead Quotes: "show a `brief` record short" | internal | loose | say `the brief record`; check CONTEXT.md:207 that Brief and the `brief` record are one thing (unverified here) |
| Context files | docs/contract.md:3229 | pi's context files (AGENTS.md, CLAUDE.md), unrelated to a Context Quotes: "**Context files** (hs-d611): pi loads context files" | upstream (pi) and internal (Context) | loose | `pi context files (AGENTS.md)` in full, never `Context files` capitalised as here |
| ticket | docs/contract.md:2855 | Jira ticket; CONTEXT.md's level table calls a Plan the `Ticket` level Quotes: "the Jira ticket after the name" | upstream (Jira); internal (level table) | loose | `Jira issue key` |
| base / base PR | docs/contract.md:2457; docs/contract.md:2460 | the Workflow's `base` branch; the base PR = the PR it is stacked on (a PR, after the rename a Workflow); the base of a pull request on GitHub (`baseRefName`) Quotes: "`base_pr` is the base PR's `<plan>/<key>`"; "the base PR before and after" | internal; upstream (git, GitHub base) | loose | `base branch`, `base Workflow` |
| workers (status line) | docs/contract.md:2687; docs/contract.md:2740 | pill `workers N/M` counts the `workers` pool's tenants (Jobs), not Worker sessions; 2740 calls `W` a count of waiting Workers Quotes: "`workers N/M · W waiting · Q queued`"; "the waiting Workers count" | internal (Worker, Resource `workers`, Tenant) | wrong | 2740: `W` counts Ready Jobs (2687), not Workers: write `the ready-Jobs count` |
| landing / checking / merging | docs/contract.md:2743 | display-only derived states, defined here and nowhere in CONTEXT.md Quotes: "A Job is **landing** while its active Attempt is checking or merging" | internal | loose | add to CONTEXT.md or the legend; `checking` collides with checks (see above) |
| Pulse | docs/contract.md:2735; docs/contract.md:2841 | cockpit widget in the Plan view (activity histogram), not in CONTEXT.md Quotes: "Pulse's error bars"; "\| Pulse bar: PR opened" | internal | loose | add a glossary entry (Cockpit, Pulse) or define on first use |
| event / hook | docs/contract.md:2689 | tmux hooks and pi's extension events are called `hook`/`event` beside cpd's Event Quotes: "`set-hook -g pane-died" | upstream (tmux, pi) | ok | capital `Event` is cpd's; keep |

**Fine (ok, one sense, no collision found):** Plan, Context (entity), Context record, Job, Attempt, Session (cpd sense), Event, Inbox, Inbox item, Lead, Worker, Manager, Escalation, Gate, Lead Branch, Locked, Dependency, Feedback item, PR poll, MCP server, Tier (model sense), `Spec`, `Cockpit`, `error`, `fork`, `park` window names, `cancel`, `archive`, `fail`, `push`, `poll`, `hold`, `record`, `decision`, `requirement`, `assumption`, `outline` (`plan outline`, `plan-outlined`), `assumption` and `finding` (used as plain words: 3 and 1 hits), and `epic`, `task`, `requirement` (absent: `rg -o -w -i '<word>'` on the part gave 0). Terms of the term list that this part never uses: Open Job, Update Job, Review Job, Ledger, Engine (capital), Brief (capital), Quick check, Axiom, Policy, Domain, Exposure, Resource, Slot, Tenant, Machine policy, Attach (capital), Resolved policy, Known owner, Fallback, Merged, PR post (the words), Pending work, Context update, Draft (capital), Dispatch, Dispatching, Grounding, Clarifying, Specifying, Dividing, Sequencing, Approving, Reviewing, Monitoring, Addressing, Mechanical checks, Distilled Context, Context store, Job kind, CPD. The lower-case or other forms (`open` Job, `update` Job, `engine`, `draft`) are rows above.


## PR occurrences

Codes: `object`, `pull request`, `phase`, `step`, `mixed`, `other`. In this part: no `step` use and no `other` use of a PR token were found (the Deploy stage's PR step is never named: `rg -n 'PR step|step.*\bPR\b'` on the part gave 6 hits, each read, none is that step). One `phase` use (2687).

### Event kinds and `data` fields that name a PR

| Kind | Where | `data` fields that name a PR | Sense | After the rename |
|---|---|---|---|---|
| `plan-phase` | docs/contract.md:2440 | `prs: [key]`, `prs: [{key, number, url}]`. Quote: "also `jobs: [key]` and `prs: [key]` (the PRs it started)" | mixed | `workflows: [key]`, entries `{key, number, url}` (key = Workflow, number and url = its pull request) |
| `plan-done` | docs/contract.md:2443 | `prs: [{key, number, url, merged_at}]`. Quote: "\| plan \| `{prs: [{key, number, url, merged_at}]}` \|" | mixed | `workflows: [...]`, same note |
| `job-added` | docs/contract.md:2444 | `pr`. Quote: "\| plan, job \| `{pr, kind, status, appended, tier, needs, review, urgent}`;" | object | `workflow` |
| `job-merged` | docs/contract.md:2446 | `pr`, `number`, `url`. Quote: "for an `open` Job also `pr`, `number` and `url`" | mixed | rename `pr` to `workflow`; `number` and `url` stay (pull request) |
| `all-merged` | docs/contract.md:2455 | `pr` (retired kind, kept for history). Quote: "\| plan \| `{pr, merged: [key], cancelled: [key]}` \|" | object | history kind: keep or rename (User decides: kinds are rows of `event_kinds`) |
| `pr-feedback` | docs/contract.md:2456 | kind: pull request; `pr`: object; `number`, `url`, `state`, `checks`, `review_decision`, `mergeable`, `merge_state`: pull request. Quote: "\| plan \| `{pr, number, url, state, checks, review_decision, mergeable, merge_state, reasons}`;" | pull request (kind), object (`pr`) | kind keeps `pr-feedback` (Spec: pull request); data `pr` becomes `workflow` |
| `pr-added` | docs/contract.md:2457 | `pr`, `base_pr`, `needs`; `head_ref` and `author` belong to an adopted pull request. Quote: "`{pr, repo, base, base_pr, title, head_ref, author, needs: [reference], appended}`" | object | `workflow-added`, `workflow`, `base_workflow` |
| `pr-edited` | docs/contract.md:2458 | `pr`. Quote: "\| plan \| `{pr, fields: [name]}`;" | object | `workflow-edited`, `workflow` |
| `pr-described` | docs/contract.md:2459 | `pr`. Quote: "\| plan \| `{pr}` \|" | mixed | the description is stored on the Workflow and shown as the pull request body: `workflow-described`? |
| `pr-base-changed` | docs/contract.md:2460 | `pr`, `from_pr`, `to_pr`, `by` (`pr start`, `pr open`), `number`, `url`. Quote: "\| plan \| `{pr, from, to, from_pr, to_pr, by}`;" | object | `workflow-base-changed`, `from_workflow`, `to_workflow`, `by` = `workflow start`, `workflow open` |
| `pr-cancelled` | docs/contract.md:2461 | `pr`, `superseded_by`. Quote: "\| plan \| `{pr}`, plus `superseded_by`," | object | `workflow-cancelled` |
| `pr-started` | docs/contract.md:2462 | `pr`, `by` = `pr start`. Quote: "\| plan \| `{pr, repo, base, lead_branch, lead_worktree, base_sha, reused, pushed, by}`;" | object | `workflow-started`, `workflow` |
| `pr-opened` | docs/contract.md:2463 | `pr`, `number`, `url`. Quote: "\| plan \| `{pr, number, url}` \|" | mixed | the Workflow becomes `open` and its pull request exists; the User decides which noun the kind names |
| `pr-merged` | docs/contract.md:2464 | `pr`, `number`, `url`. Quote: "\| plan \| `{pr, number, url, merged_at}` \|" | mixed | as `pr-opened` |
| `incident-started` | docs/contract.md:2491 | `until_pr`, flag `--until-pr`. Quote: "`until_pr` the PR key or null" | object | `until_workflow`, `--until-workflow` |
| `incident-plans` | docs/contract.md:2492 | `until_pr`. Quote: "`until_pr` the PR key or null" | object | `until_workflow` |
| `incident-ended` | docs/contract.md:2493 | `cause` value `"until-pr"`. Quote: "`cause` is `"manual"`" | mixed | `"until-workflow"`; the trigger is the Workflow's merge |
| `error (codes)` | docs/contract.md:2495 | `details.pr`, `pr_start_failed`, `cleanup_failed` `details: {plan, pr, ...}`, `jira_required` (`pr.jira_required`). Quote: "`details.pr` names the PR" | object (`jira_required`: mixed) | `details.workflow`, `workflow_start_failed`; `pr.jira_required` is a policy key |
| `pr-ready (Inbox kind)` | docs/contract.md:2807 | Inbox kind about a pull request ready to post. Quote: "the `pr-ready` glyph: red, yellow, cyan or" | pull request | keep |

Event kinds that name no PR but are not affected: every `job-*`, `attempt-*`, `escalation*`, `session-*`, `plan-*` (other than `plan-phase`, `plan-done`), `context-*`, `inbox-*`, `mcp-attached`, `review-*`, `records-retired`, `docs-decided`, `watch-*`.

### Labels the cockpit shows

| Label | Where | Sense | After the rename |
|---|---|---|---|
| Pane title `Deploy` (widgets) | docs/contract.md:2613 | other: the Deploy column, a list of pull requests of every owner. Quote: "\| `@cpd_view = widgets` \| `Deploy` \|" | keep; see Terms `Deploy` |
| Pane titles `Context`, `Plan` | docs/contract.md:2611 | other: pane named for a CPD stage. Quote: "\| `@cpd_view = tree` \| `Context` \|" | keep |
| Deploy column header pill `PRs` and its count | docs/contract.md:2852 | pull request. Quote: "\| Section title pill `PRs` and its count pill \|" | keep `PRs` |
| Deploy column empty line ` no open PRs` | docs/contract.md:2870 | pull request. Quote: "\| ` no open PRs` \| no \| fg 8 \| palette: muted \|" | keep |
| Deploy column first line ` owner/repo#12` | docs/contract.md:2853 | pull request (`owned` = a Plan tracks it). Quote: "PR first line ` owner/repo#12`, owned / not owned" | keep; `owned` needs a noun, see Terms `owner` |
| Deploy column second line `open` / `draft` | docs/contract.md:2856 | pull request (GitHub state, GitHub draft). Quote: "\| PR second line: `open`, owned / not owned \|" | keep |
| Deploy column menu `Merge`, `Copy Link`, `Go to Lead` | docs/contract.md:2873 | pull request. Quote: "PR menu entries `Merge`, `Copy Link`, `Go to Lead`" | keep |
| Plan view `PR box` (border, exit, landing) | docs/contract.md:2824 | object (the Workflow's box). Quote: "\| PR box border and its `┬` exit," | `Workflow box` |
| Plan view box title: key, repo, base | docs/contract.md:2825 | object. Quote: "PR box title: the key (bold), ` · repo" | `Workflow box title` |
| Plan view box title: `#12`, `open`, `merged`, `closed`, `conflict`, `updating` | docs/contract.md:2826 | mixed (pull request facts on a Workflow's box). Quote: "\| PR box title: `#12` \|" | `Workflow box title`; the facts keep their words |
| Plan view box status before a pull request exists | docs/contract.md:2833 | object. Quote: "PR status before a pull request exists (`waiting`, `dispatch`)" | `Workflow status`; `waiting` also names a Session state (Terms) |
| Plan view foot line `no PR yet: cpd pr add` | docs/contract.md:2836 | object. Quote: "`no PR yet: cpd pr add`" | `no Workflow yet: cpd workflow add` |
| Legend: `PR states` | docs/contract.md:2847 | mixed. Quote: "PR states, checks, tiers, Pulse samples" | `Workflow states` and `pull request checks` |
| Inbox kinds `plan-review`, `review-waiting`, `pr-ready`, `review-requested`, `deploy` | docs/contract.md:2809 | other: Plan review; Session blocked on a review; pull request ready; team review; `deploy` (unverified: meaning not in this part). Quote: "`plan-review`, `review-waiting`, `pr-ready`, `review-requested`, `deploy`" | keep `pr-ready`; define `deploy` |
| Inbox tiers red, yellow, magenta, cyan, white | docs/contract.md:2807 | other: urgency rank (collides with Tier). Quote: "Tier glyph: red / yellow / magenta / cyan / white" | `urgency` |
| Inbox notice kinds `curate-docs`, `curate-retire` | docs/contract.md:2740 | other: Context curation items. Quote: "curate-docs and curate-retire items" | keep |
| Status line `workers N/M · W waiting · Q queued` | docs/contract.md:2687 | other: pool occupancy, Ready Jobs, queued Jobs. Quote: "`workers N/M · W waiting · Q queued`" | `W ready` |
| Status line `Incident Mode: ` | docs/contract.md:2644 | other. Quote: "reading `Incident Mode: `" | keep |
| Job pills draft, queued, running, landing, merged, failed, cancelled | docs/contract.md:2743 | other: Job status plus the derived `landing`. Quote: "**Job pills** (the Plan view's cards, the legend's samples," | keep |
| Lead dot by phase: grounding, plan, dispatch, deploy, done | docs/contract.md:2781 | phase (Plan phase), used correctly. Quote: "working Lead's dot, plan phase (spinner)" | keep |
| Counters: queued, draft (phase `plan`), running, landing, merged, failed | docs/contract.md:2789 | other: Job counts. Quote: "Counter: draft (phase `plan`)" | keep |
| Pulse bars: PR opened, Job or PR merged | docs/contract.md:2841 | mixed. Quote: "Pulse bar: PR opened, escalation, any other activity" | `Workflow opened`, `Job or Workflow merged` |

### Count per group of the part

| Where (section) | Count | Sense (code) | Quote (at most 15 words) | After the rename |
|---|---|---|---|---|
| 5.1 Rules (2422-2427) | 2 | pull request | docs/contract.md:2424: "only to record data (metrics, a `pull_requests` row that changes no PR's state," | unchanged (`PR` keeps this meaning) |
| 5.1 Rules (2422-2427) | 1 | mixed | docs/contract.md:2424: "a `pull_requests` row that changes no PR's state, a `pr_posts` row) append no" | judgement: see the individual rows below |
| 5.2 Kinds (2428-2503) | 104 | object | docs/contract.md:2440: "`dispatch → deploy` (when the last PR not yet open opens, A10), `deploy" | `Workflow` (mechanical: rule R1) |
| 5.2 Kinds (2428-2503) | 19 | pull request | docs/contract.md:2446: "`url`, and its `text` says the pull request is open (hs-d340: it is" | unchanged (`PR` keeps this meaning) |
| 5.2 Kinds (2428-2503) | 21 | mixed | docs/contract.md:2446: "a `work` Job of an `open` PR whose stored `feedback` (§2.2 "Feedback item")" | judgement: see the individual rows below |
| 5.3 Delivery (2504-2552) | 9 | object | docs/contract.md:2507: "has one Lead for all its PRs (R34): every PR Event (`pr-*`, `pr-feedback`)," | `Workflow` (mechanical: rule R1) |
| 5.3 Delivery (2504-2552) | 3 | pull request | docs/contract.md:2507: "PRs (R34): every PR Event (`pr-*`, `pr-feedback`), and an `error` about a PR" | unchanged (`PR` keeps this meaning) |
| 5.3 Delivery (2504-2552) | 5 | mixed | docs/contract.md:2507: "for all its PRs (R34): every PR Event (`pr-*`, `pr-feedback`), and an `error`" | judgement: see the individual rows below |
| 6.1 The server (2567-2692) | 1 | object | docs/contract.md:2687: "Jobs (§3.2: `queued`, every Dependency `merged`, PR `dispatch` or `open`, Plan in `dispatch`" | `Workflow` (mechanical: rule R1) |
| 6.1 The server (2567-2692) | 1 | phase | docs/contract.md:2687: "or `open`, Plan in `dispatch` or `pr`: the Jobs E3 would start if" | `deploy` |
| 6.1.1 Dango palette: elements and formats (2693-2964) | 10 | object | docs/contract.md:2721: "cells of a drawn buffer: the PR boxes (by drawing each through a" | `Workflow` (mechanical: rule R1) |
| 6.1.1 Dango palette: elements and formats (2693-2964) | 39 | pull request | docs/contract.md:2703: "pane uses it: the Deploy column's `PRs` header, the legend's and the logs" | unchanged (`PR` keeps this meaning) |
| 6.1.1 Dango palette: elements and formats (2693-2964) | 11 | mixed | docs/contract.md:2736: "**success, completed**: merged Jobs and PRs, passed checks, Pulse's merge bars" | judgement: see the individual rows below |
| 6.1.1 pi theme and pi chat rows (2965-3099) | 2 | object | docs/contract.md:3090: "2026-10-22; replaces the padded label of PR `chatlive`: `pill(RUNNING, ` ${message} `)` with" | `Workflow` (mechanical: rule R1) |
| 6.1.1 pi theme and pi chat rows (2965-3099) | 2 | mixed | docs/contract.md:3022: "not italic (the Thinking fold's body stays upright, a later PR enforces it)" | judgement: see the individual rows below |
| 6.3-6.5 tmux commands, where, priority (3158-3215) | 7 | mixed | docs/contract.md:3208: "and the Gate (`pre-merge`) runs of `pr open`, `pr rebase`, `pr push` and" | judgement: see the individual rows below |
| 6.6 How cpd starts pi (3216-3257) | 1 | object | docs/contract.md:3241: "in the repo of its Job's PR. The project settings read below (`<cwd>/.pi/settings.json`)" | `Workflow` (mechanical: rule R1) |
| 6.9-6.11 compact-tools, input widget, balloons, ask_user_question, MCP (3322-3490) | 1 | object | docs/contract.md:3361: "on a folded block did nothing; PR `expand` of Plan italic pins it" | `Workflow` (mechanical: rule R1) |

### Every `mixed` and `phase` token, one by one

| Where | Count | Sense | Quote | Why it is mixed (or phase) | After the rename |
|---|---|---|---|---|---|
| docs/contract.md:2424 (token 2) | 1 | mixed | "a `pull_requests` row that changes no PR's state, a `pr_posts` row) append no" | PR's state: a pull request's state, or a Workflow's status? the text does not say | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2446 (token 3) | 1 | mixed | "a `work` Job of an `open` PR whose stored `feedback` (§2.2 "Feedback item")" | `open` is the Workflow status; the stored feedback is the pull request's | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2446 (token 4) | 1 | mixed | "not empty, its `text` ends with `PR <key> still has <n> open review" | text names a Workflow key and the pull request's review comments | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2446 (token 5) | 1 | mixed | "has <n> open review comments: cpd pr view --pr <key>, then cpd pr" | `pr view` shows a Workflow and its pull request's feedback | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2456 (token 4) | 1 | mixed | "CANCELLED or pending checks) and the PR's `pr.feedback.checks` is `any` (§8.3, hs-d292), its" | policy key `pr.feedback.checks` governs the pull request's checks, scoped per Workflow | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2456 (token 5) | 1 | mixed | "or pending checks) and the PR's `pr.feedback.checks` is `any` (§8.3, hs-d292), its review" | policy key `pr.feedback.checks`: the checks are the pull request's, the key is a policy domain named `pr` | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2456 (token 6) | 1 | mixed | "state became `closed` (from `open`), **the PR has a `pr_posts` row, is `open`," | a Workflow (`open`) that has a pull request post and a pull request review | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2456 (token 14) | 1 | mixed | "and wakes the Lead again. `text`: `PR <key> (#<n>) of Plan <p> needs" | text prints a Workflow key and the pull request number `#<n>` | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2456 (token 15) | 1 | mixed | "<reasons>. Read the details with cpd pr view --plan <p> --pr <key>.`, the" | `pr view` shows a Workflow and its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2456 (token 17) | 1 | mixed | "of open comments and says `cpd pr view` (§2.2 "Feedback item"); `pr.feedback.checks` does" | `pr view` shows a Workflow and its pull request's comments | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2456 (token 18) | 1 | mixed | "`cpd pr view` (§2.2 "Feedback item"); `pr.feedback.checks` does not apply to the `approved`" | policy key `pr.feedback.checks` again (same as 2456.5) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2459 (token 1) | 1 | mixed | "`pr-described`" | Event kind `pr-described`: the description is stored on the Workflow and becomes the pull request body | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2459 (token 2) | 1 | mixed | "`pr describe` (commands.md §9.5a) in its transaction, only when the stored description changed" | `pr describe` stores a description on a Workflow for its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2460 (token 9) | 1 | mixed | "PR's merge), E7, `pr start` or `pr open` unstack a `waiting` or `dispatch`" | `pr open` acts on a Workflow and opens its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2460 (token 23) | 1 | mixed | "`by` is `poll`, `engine`, `pr start`, `pr open` or `open job` (an `open`" | `pr open` acts on a Workflow and opens its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2463 (token 1) | 1 | mixed | "`pr-opened`" | Event kind `pr-opened`: the Workflow reaches `open` because its pull request is opened | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2463 (token 3) | 1 | mixed | "PR's `open` Job merges (§7.7), or `pr open` (`dispatch → open`, the repair)" | `pr open` acts on a Workflow and opens its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2464 (token 1) | 1 | mixed | "`pr-merged`" | Event kind `pr-merged`: the Workflow reaches `merged` because the pull request merged | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2493 (token 1) | 1 | mixed | "`cpd incident end` or auto-end when `until-pr` merges" | `until-pr` auto-ends when the Workflow merges, which is when its pull request merges | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2495 (token 3) | 1 | mixed | "for a PR whose policy sets `pr.jira_required` and that has no stored `jira`)," | policy key `pr.jira_required` concerns the pull request's Jira, scoped per Workflow | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2500 (token 1) | 1 | mixed | "its curations and proposals (the next PR, `distill`; added to `event_kinds` by migration" | `the next PR, distill`: a unit of delivery of a Plan (a Workflow) that ends as a pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2502 (token 2) | 1 | mixed | "Session failed (DEFECT 2); 16 until `pr-described`, pro-d94: the adapter shows that kind," | Event kind `pr-described` (same as 2459.1) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2507 (token 2) | 1 | mixed | "for all its PRs (R34): every PR Event (`pr-*`, `pr-feedback`), and an `error`" | `pr-*` covers `pr-feedback` (pull request) and `pr-started` (Workflow) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2508 (token 1) | 1 | mixed | "the cursor of kinds `plan-created`, `plan-phase`, `pr-opened`, `pr-merged`, `job-failed` and `plan-done` of the" | Event kind `pr-opened` (same as 2463.1) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2508 (token 2) | 1 | mixed | "cursor of kinds `plan-created`, `plan-phase`, `pr-opened`, `pr-merged`, `job-failed` and `plan-done` of the Context" | Event kind `pr-merged` (same as 2464.1) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2524 (token 1) | 1 | mixed | "`pr-merged`" | Event kind `pr-merged` (same as 2464.1) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2526 (token 1) | 1 | mixed | "`pr-described`" | Event kind `pr-described` (same as 2459.1) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2687 (token 2) | 1 | phase | "or `open`, Plan in `dispatch` or `pr`: the Jobs E3 would start if" | Plan phase `pr` (the old phase, now `deploy`) | `deploy` |
| docs/contract.md:2736 (token 1) | 1 | mixed | "**success, completed**: merged Jobs and PRs, passed checks, Pulse's merge bars" | green marks merged Jobs and merged Workflows and pull requests alike | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2740 (token 3) | 1 | mixed | "bar), pending checks, Pulse's activity and PR-opened bars, the waiting Workers count (the" | Pulse bar for `pr-opened`: a Workflow opens its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2826 (token 1) | 1 | mixed | "PR box title: `#12`" | Plan view box title `#12`: the number of the pull request, in a box that is the Workflow's | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2827 (token 1) | 1 | mixed | "PR box title: state `open`" | box title state `open`: Workflow status and pull request state coincide | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2827 (token 2) | 1 | mixed | "fg 15; fg 11 when the PR is ready to merge" | `ready to merge` is `merge_state` clean, a pull request fact, on a Workflow's box | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2828 (token 1) | 1 | mixed | "PR box title: `merged`" | box title `merged`: Workflow status and pull request state coincide | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2829 (token 1) | 1 | mixed | "PR box title: `closed`, its Lead handles it / asks the User" | box title `closed`: the pull request state, shown on a Workflow's box | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2830 (token 1) | 1 | mixed | "PR box title: `conflict` and `updating` (every box PR has a Lead)" | box title `conflict` and `updating`: pull request mergeability, update Job of the Workflow | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2841 (token 1) | 1 | mixed | "Pulse bar: PR opened, escalation, any other activity" | Pulse bar for `pr-opened` (same as 2740.3) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2842 (token 1) | 1 | mixed | "Pulse bar: Job or PR merged" | Pulse bar for Job merged or `pr-merged` (a Workflow, its pull request) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:2847 (token 1) | 1 | mixed | "popup: swatches, Job sample pills, counters, Dots, PR states, checks, tiers, Pulse samples" | legend samples the PR box states (Workflow) and checks (pull request) | decide per row: Workflow, pull request, or both named |
| docs/contract.md:3022 (token 1) | 1 | mixed | "not italic (the Thinking fold's body stays upright, a later PR enforces it)" | `a later PR`: a later delivery unit of the Plan (Workflow) or a later pull request; cannot tell | decide per row: Workflow, pull request, or both named |
| docs/contract.md:3028 (token 1) | 1 | mixed | "block's body is upright whatever the hook does (a later PR enforces it)." | same as 3022.1 | decide per row: Workflow, pull request, or both named |
| docs/contract.md:3208 (token 1) | 1 | mixed | "and the Gate (`pre-merge`) runs of `pr open`, `pr rebase`, `pr push` and" | `pr open`/`pr rebase`/`pr push` act on a Workflow's Lead Branch and its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:3208 (token 2) | 1 | mixed | "Gate (`pre-merge`) runs of `pr open`, `pr rebase`, `pr push` and the repair" | `pr open`/`pr rebase`/`pr push` act on a Workflow's Lead Branch and its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:3208 (token 3) | 1 | mixed | "runs of `pr open`, `pr rebase`, `pr push` and the repair `pr open`" | `pr open`/`pr rebase`/`pr push` act on a Workflow's Lead Branch and its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:3208 (token 4) | 1 | mixed | "rebase`, `pr push` and the repair `pr open` (§7.6) or a Lead's `cpd" | `pr open`/`pr rebase`/`pr push` act on a Workflow's Lead Branch and its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:3208 (token 5) | 1 | mixed | "open` (§7.6) or a Lead's `cpd pr push` / `pr rebase` / `pr" | `pr open`/`pr rebase`/`pr push` act on a Workflow's Lead Branch and its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:3208 (token 6) | 1 | mixed | "or a Lead's `cpd pr push` / `pr rebase` / `pr open` repair." | `pr open`/`pr rebase`/`pr push` act on a Workflow's Lead Branch and its pull request | decide per row: Workflow, pull request, or both named |
| docs/contract.md:3208 (token 7) | 1 | mixed | "or a Lead's `cpd pr push` / `pr rebase` / `pr open` repair." | `pr open`/`pr rebase`/`pr push` act on a Workflow's Lead Branch and its pull request | decide per row: Workflow, pull request, or both named |

## Fix list

**Rules (mechanical, no row each).**

- R1. Every `object` token is a Workflow after the rename: `PR`/`PRs` -> `Workflow`/`Workflows`; `pr add|edit|start|cancel|describe` -> `workflow ...` (commands); `--pr <key>` -> `--workflow <key>`; Event kinds `pr-added`, `pr-edited`, `pr-base-changed`, `pr-cancelled`, `pr-started` -> `workflow-*`; data fields `pr`, `prs`, `base_pr`, `from_pr`, `to_pr`, `until_pr`, `details.pr`, `jobs.pr` -> `workflow...`. Count: 128 tokens (per group in the table above).
- R2. Every `pull request` token stays as written. Count: 63.
- R3. The Plan view's `PR box` (10 rows, 2824-2833, and 2721) becomes `Workflow box`; the Deploy column's `PRs` header and `PR` rows (2851-2875) stay.
- R4. Event kinds are rows of `event_kinds` (5.2): a rename is a migration plus a `PROTOCOL` raise (5.2, protocol paragraph at 2502); this audit does not decide that. The table at 2440-2464 is the list.

**Judgement rows.**

| path:line | what to change | why |
|---|---|---|
| docs/contract.md:2687 | `Plan in dispatch or deploy` (quote: "Plan in `dispatch` or `pr`") | stale `phase` use: the phase `pr` is retired (CONTEXT.md:15) |
| docs/contract.md:2740 | `the ready-Jobs count` (quote: "the waiting Workers count") | wrong: `W` counts Ready Jobs (2687), not Workers |
| docs/contract.md:2687 | rename the label `waiting` to `ready` (quote: "**waiting**) is the number of Ready Jobs") | `waiting` is also the Session state (2469) |
| docs/contract.md:3260 | `one per Plan` (quote: "one engine instead of one per Workflow") | stale: cpd 1's Workflow is retired; the new Workflow is another object |
| docs/contract.md:2443 | keep, add `cpd 1's name for Plan` (quote: "cpd 1's `workflow-done`") | history, but a bare `workflow-done` reads as the new Workflow |
| docs/contract.md:2543 | keep (history of renamed kinds) (quote: "`workflow-done` → `plan-done`") | history; the new Workflow makes it ambiguous: say `cpd 1's Workflow (now Plan)` |
| docs/contract.md:2506 | `§6.6` (quote: "loaded in every pi session cpd starts, §6.5") | wrong cross-reference: §6.5 is Priority; pi's start is the first §6.6 |
| docs/contract.md:2512 | `§6.6` (quote: "`steeringMode`; §6.5)") | wrong cross-reference (queue modes are in §6.6 `How cpd starts pi`) |
| docs/contract.md:2571 | `§6.6` (quote: "through its `Spawn`, §6.5") | wrong cross-reference |
| docs/contract.md:3289 | `§6.6` (quote: "-- @<first message file>` (§6.5)") | wrong cross-reference |
| docs/contract.md:3352 | `§6.6` (quote: "as §6.5 compares packages") | wrong cross-reference (the package identity rule is in `[-e …]` of §6.6) |
| docs/contract.md:3478 | `§6.6` (quote: "through the same log as the skip rule of §6.5") | wrong cross-reference |
| docs/contract.md:2689 | qualify: `§6.6 The engine's place` (quote: "**The hooks** (§6.6).") | two headings are numbered 6.6 (3216 and 3258): the reference is ambiguous |
| docs/contract.md:3258 | renumber (a second 6.6 follows `6.6 How cpd starts pi`) (quote: "### 6.6 The engine's place (D5)") | duplicate section number; §6.7 and §6.8 references depend on it |
| docs/contract.md:3144 | `tmux sessions, windows and panes` (quote: "### 6.2 Sessions, windows and panes") | `Sessions` here are tmux's, not cpd Sessions |
| docs/contract.md:2735 | `the Inbox's red urgency` (quote: "the Inbox's red tier") | `tier` is the model Tier elsewhere |
| docs/contract.md:3229 | `pi context files (AGENTS.md)` (quote: "**Context files** (hs-d611): pi loads context files") | collides with Context |
| docs/contract.md:2736 | `merged Jobs and Workflows`, or `pull requests` if GitHub's is meant (quote: "merged Jobs and PRs") | mixed: cannot tell from the text |
| docs/contract.md:2847 | `Workflow states, pull request checks, urgencies` (quote: "PR states, checks, tiers, Pulse samples") | mixed |
| docs/contract.md:2857 | `pull request second line: draft pull request` (quote: "\| PR second line: `draft`, owned / not owned \|") | draft Job vs draft pull request |
| docs/contract.md:2855 | `Jira issue key` (quote: "the Jira ticket after the name") | `ticket` is the Plan level in CONTEXT.md |
| docs/contract.md:2743 | define `checking` once in CONTEXT.md or the legend (quote: "is shown as `checking`") | a new term, collides with checks of the pull request |
| docs/contract.md:2809 | define Inbox kind `deploy` (quote: "`review-requested`, `deploy`") | unverified: no definition of the kind in this part |

The `mixed` tokens (47) have one row each in the last table of `## PR occurrences`; their fix is the User's choice of noun, so they are not repeated here.


## Counts

| What | Number |
|---|---|
| files audited | 1 (`docs/contract.md`) |
| lines read | 1071 (2420-3490) |
| headings in the part | 23 |
| rows in `## Terms` | 32 (plus one summary paragraph) |
| rows in the Event-kind table | 19 |
| rows in the cockpit-label table | 22 |
| rows in the count-per-group table | 18 |
| rows in the individual `mixed`/`phase` table | 48 |
| rows in `## Fix list` (judgement) | 23 (plus 4 rules) |
| PR tokens, all | 239 on 88 lines |
| `object` | 128 |
| `pull request` | 63 |
| `mixed` | 47 |
| `phase` | 1 |
| `step` | 0 |
| `other` | 0 |
| `Workflow` / `workflow` words | 3 |
