# Glossary audit: contract sections 1 and 2 (principles, model)

Audit of `docs/contract.md` lines 1 to 748 (the line before `## 3. State machines`, which is line 749). Report only: no file but this one was changed. Decisions behind the audit: Context records pro-d136 and pro-d138 (2026-10-28), as the Job's Spec states: 'PR' means only the GitHub pull request; the cpd object that is called PR today becomes a Workflow. Sense codes: `object`, `pull request`, `phase`, `step`, `mixed`, `other`. Where the text did not decide, the code is `mixed` and the row says why. Nothing here is decided for the User: every proposal is a proposal.

## Coverage

| File | Lines | Read |
|---|---|---|
| docs/contract.md | 1-22 (title, status, sources, contents) | in full |
| docs/contract.md | 23-39 (1.1 What cpd 2 is) | in full |
| docs/contract.md | 40-91 (1.2 Output, exit codes, test overrides, code tables) | in full |
| docs/contract.md | 92-100 (1.3 Conventions) | in full |
| docs/contract.md | 101-200 (1.4 `$CPD_HOME`: file tree, locks, holder record) | in full, with the read-through aimed at words and PR senses; the lock mechanics were not checked against section 6 or 7 (other parts) |
| docs/contract.md | 201-226 (2.1 Identifiers) | in full |
| docs/contract.md | 227-345 (2.2 Context, Context record, Plan, PR, Pull request) | in full |
| docs/contract.md | 347-387 (2.2 Feedback item, PR post, Plan review) | in full |
| docs/contract.md | 388-474 (2.2 Job and Spec, Job kinds, Dependency, Attempt) | in full |
| docs/contract.md | 475-555 (2.2 Escalation, Session, Asleep, Archived, Deleted) | in full; the Deleted rules (535-555) are long and were read for terms, not for the correctness of the delete design |
| docs/contract.md | 556-657 (2.2 Event, Inbox item and its rules) | in full; the Inbox rule paragraphs (631-657) and the team-review rule were read for terms and PR senses, not for the hermes port details |
| docs/contract.md | 658-681 (2.2 Incident) | in full |
| docs/contract.md | 682-689 (2.3 Fingerprints) | in full |
| docs/contract.md | 690-724 (2.4 Relations) | in full |
| docs/contract.md | 725-748 (2.5 PRs in commands and views) | in full |
| CONTEXT.md | 1-35 | read only to check the Plan phase entries (Dispatch, PR (phase)); the rest of CONTEXT.md was not read, so every term listed under 'fine' below is judged from the contract text alone (unverified against CONTEXT.md) |
| docs/contract.md | 749-4258 | not read: other Jobs audit it |

Skimmed: nothing; the whole part was read with the read tool in chunks of 120-200 lines (lines 1-200, 201-350, 351-500, 501-630, 631-748). A few lines of 2.2 Context record and Plan and Incident were read once and are stated here as read, not re-verified.

Commands behind the counts (run from the repository root; `/tmp/part.md` is `head -748 docs/contract.md`):

```sh
head -748 docs/contract.md > /tmp/part.md
rg -n '^## 3\. State machines' docs/contract.md          # 749: the part ends at 748
head -748 docs/contract.md | wc -l                         # 748
head -748 docs/contract.md | rg -o '\bPRs?\b' | wc -l      # 335 occurrences of the word PR or PRs
head -748 docs/contract.md | rg -o -i 'pull requests?' | wc -l   # 99 occurrences of 'pull request(s)'
head -748 docs/contract.md | rg -P -o '(?<![\w-])(`?cpd )?pr (add|edit|open|cancel|start|rebase|push|view|poll|post|reply|merge|describe)\b|--pr\b|--until-pr|(?<![\w])pr[_-][a-z][a-z_-]*|(?<![\w`.])`pr`|\bprs\b|(?<![\w])[a-z_]*_pr(_[a-z]+)?\b|\bpull_requests?\b|\bPR_NEED\b|<pr>|\bpr\.[a-z_.]+' | wc -l   # 239 identifier occurrences
head -748 docs/contract.md | rg -n 'PR step|PR phase'   # 0: no `step` use of PR in this part
head -748 docs/contract.md | rg -n -i 'workflow'          # the 'workflow' uses in Terms
head -748 docs/contract.md | rg -n '`pr`' | rg 'dispatch'  # line 593: the stale phase `pr`
```

Limits (all unverified claims are labelled where they appear): the identifier regex finds the tokens it lists; a `pr` identifier of another shape (for example inside a longer JSON key) is not counted. The sense of each occurrence is judged from the text of this part only; where a sense lives in another part (for example `pr_closed`, `pr_exists`: section 7.6) the code is `mixed` and the row says so.

## Terms

| Term | Where (path:line) | Sense as used here | Collision | Verdict | Proposed wording |
|---|---|---|---|---|---|
| PR (entity) | docs/contract.md:3, 36, 281, 306, 716 | the cpd object (one repo's part of a Plan; maps to one pull request); line 306 also defines it against 'pull request' | internal: pull request (306 contrasts them in one sentence). upstream: GitHub 'PR' = pull request | wrong (after pro-d136/pro-d138) | Workflow throughout; delete the sentence at 306 or turn it into 'A Workflow maps to one pull request but is not it' |
| Workflow (cpd 1 name of Plan) | docs/contract.md:210, 267 | history: cpd 1's name for Plan (retired by D3) | internal: Workflow is now the renamed PR object, so 'Plan = cpd 1's Workflow' reads as a contradiction | stale | say 'cpd 1 called a Plan a workflow (retired by D3); that word now names the Workflow object' or drop the parenthesis |
| workflow (cpd 1 identifiers) | docs/contract.md:94, 103, 290 | cpd 1 history: flag, directory and command, each already labelled cpd 1 | internal: same word as the new Workflow (CLI noun `cpd workflow …`) | loose | keep (history keeps old words); no change |
| GitHub Actions workflow | docs/contract.md:604, 639, 142 | a GitHub Actions workflow run awaiting approval (Inbox kind `deploy`; JSON key `workflow` of `deploy-runs.json`) | upstream: GitHub Actions. internal: the new Workflow | loose | write 'GitHub Actions workflow run' in full at 639; JSON key `workflow` at 142 needs a name that cannot be read as a cpd Workflow (for example `actions_workflow`; the User decides) |
| phase `pr` | docs/contract.md:593 | the old Plan phase called 'PR' (now `deploy`) | internal: PR object; Plan phase list at 274 has no `pr` | stale | `dispatch` or `deploy` |
| plan | docs/contract.md:274, 253, 377, 685 | Plan (object), phase `plan`, Context record kind `plan`, command group `plan` | internal: four meanings of one word, two inside one sentence at 685 | overloaded | keep; write 'phase `plan`' and 'record kind `plan`' every time (CONTEXT.md writes **Plan** (phase)) |
| dispatch / Dispatch | docs/contract.md:274, 292, 291, 318, 593 | Plan phase `dispatch`; PR status `dispatch` (its Jobs are running); the cockpit's Dispatch column (pull requests) | internal: phase and Workflow status share the word; 291 uses both in one sentence ('in phase `dispatch`' next to a status) | overloaded | keep the phase. Workflow status needs its own word (for example `started`); the User decides, because status values are stored in table `prs` |
| deploy / Deploy | docs/contract.md:274, 604, 142, 343 | Plan phase `deploy`; Inbox kind `deploy` (workflow run awaiting approval); the Deploy column and menu of the cockpit | internal: the phase is for open pull requests, the Inbox kind is for GitHub Actions deploys | overloaded | keep the phase; name the Inbox kind and file for what they are (for example `deploy-approval`); the User decides |
| open | docs/contract.md:292, 328, 435, 483, 261 | Workflow status `open`; pull request state `open`; Job kind `open` (Open Job); Escalation status `open`; a question is open | internal: five meanings. upstream: GitHub 'open' pull request | overloaded | qualify in prose ('Workflow status `open`', 'the Open Job'); 'open the pull request' for the verb; the User decides whether Workflow status gets its own word |
| merged / merge | docs/contract.md:292, 328, 415, 435, 449 | Job Merged (into the Lead Branch); Workflow `merged` (pull request squash-merged on GitHub); the open Job 'merges the base' into the Lead Branch; pull request state `merged`; `merge_state` | upstream: git merge and GitHub merge are different operations in one word | overloaded | write 'merged into the Lead Branch' (Job) and 'merged on GitHub' (Workflow, pull request); the contract does so only in places |
| draft | docs/contract.md:327, 418, 435, 625 | Job status `draft` (stub draft); pull request `draft` flag; `merge_state` value `draft`; Inbox status `draft` | upstream: GitHub draft pull request. internal: draft Job | overloaded | 'draft Job' versus 'draft pull request' every time; no change to values |
| review | docs/contract.md:329, 359, 373, 396, 334 | Plan review (User approves draft Jobs); review Job (fork of the Lead writing findings); Feedback item kind `review` and `review_decision` (GitHub); Inbox `review-requested` (team Slack) | internal: four meanings. upstream: GitHub review | overloaded | keep 'Plan review' and 'review Job' as two terms; reserve 'review' alone for GitHub; Job column `review` could be `review_job` (User decides) |
| check / checks | docs/contract.md:330, 431, 434, 298 | pull request CI checks; Quick check; 'a check that cpd reads' (kinds); SQL CHECK constraint | upstream: GitHub checks, SQLite CHECK | loose | no change; 'CI checks' for GitHub's and 'Quick check' (capitalised) for cpd's |
| context (Spec field) | docs/contract.md:390, 403 | Spec field `context` (background text of a Job) | internal: Context (entity) differs from the lowercase field only by the capital letter; CONTEXT.md reserves 'Context' for the entity | overloaded | rename the Spec field (for example `background`); User decides; touches Spec files, so not a rewording |
| session | docs/contract.md:32, 453, 465 | pi Session (object) versus a tmux session (32) versus 'Worker session' in lower case (453) | upstream: tmux session, pi session | loose | 'tmux session' for tmux's; 'Session' (capital) for cpd's; 'Worker Session' at 453 |
| tier (palette) | docs/contract.md:577 | colour names of Inbox urgency, called 'tier names' | internal: Tier means a model tier (S1..); the same word names colours | wrong | 'urgency names' (or 'colour names') |
| owner | docs/contract.md:211, 241, 614 | GitHub login of a Context (policy key); GitHub repo owner; OWNER = the newest Plan whose Workflow holds a pull request | upstream: GitHub owner. internal: 614 uses 'owner' for a Plan | overloaded | rename OWNER at 614 to 'owning Plan' |
| Locked / lock | docs/contract.md:172, 277, 377, 390 | Locked (Job and fields, set at `plan lock`); file locks (`flock`, section 1.4); command `plan lock`; `locked_at` | internal: two unrelated meanings; upstream: flock | overloaded | 'Locked' (capital) for the cpd state only; 'file lock' for flock |
| state / status / phase | docs/contract.md:274, 292, 328, 504 | Plan `phase`, PR/Job/Attempt `status`, pull request `state`, Session `state` | internal: four words for 'where it is in its life' | loose | keep (columns are stored); define the four words once in CONTEXT.md |
| waiting / blocked | docs/contract.md:292, 504, 529, 577 | Workflow status `waiting` (needs unmet) versus Session state `waiting` (505: waiting for a Worker answer); Session state `blocked` (dialog) versus `archive_blocked` versus 'blocked work' (red urgency) | internal: CONTEXT.md/AGENTS: 'blocked' means needs the human; here a Session can be `blocked` on a tool dialog and 'blocked work' is red | overloaded | keep Session states; for Workflow `waiting` rename only with the status words (see dispatch) |
| Ready | docs/contract.md:343, 371, 442, 444 | Ready (a Job can start), ready to merge (pull request), ready to post (pull request), `ready_to_post` (a removed reason) | internal | loose | capital 'Ready' for Jobs only, as now; no change |
| post | docs/contract.md:367, 535, 649 | PR post (Slack message about a pull request), board post (Session board), Slack posts of the team review | internal: three meanings; upstream: GitHub 'post' a comment (`pr reply`) | loose | 'pull request post' stays with the pull request; no change beyond the rename |
| pr / pull_request fields | docs/contract.md:300, 301, 395 | Workflow field `pr` (json: number, url, opened_at) and field `pull_request` (row id) both point at the pull request; Job field `pr` names the Workflow key | internal: the same name `pr` holds a pull request's data on prs and a Workflow key on jobs | overloaded | Job `pr` becomes `workflow` (object rule); the json `pr` of the Workflow and `pull_request` should be one field (User decides which) |
| Jira task / ticket / Epic | docs/contract.md:233, 267, 299 | Plan 'optionally a Jira task'; Workflow 'own Jira ticket'; Context 'Jira Epic' | upstream: Jira. internal: CONTEXT.md level table says Epic=Context, Ticket=Plan, Task=Job, so 'Jira task' for a Plan contradicts it | wrong | 267: 'a Jira ticket' (the Plan is the Ticket level) |
| `until_pr` format | docs/contract.md:670, 312 | incident column naming a PR as `<plan>/<key>` | internal: 2.1 and 312 say a PR key has no `/` and a bare key is same-Plan only; `<plan>/<key>` is not a defined form | wrong | rename `until_pr` to `until_workflow`; define the Plan-qualified form in 2.2 'A PR reference' or say 'Plan id and key' |
| `--base` reference form | docs/contract.md:312, 732, 734 | how `--base` names a base PR | internal: 312 says bare `<key>`; 732 and 734 say `<plan>/<key>` | wrong | make 2.5 agree with 312 (bare `<key>`) |
| duplicate `pr add` / `pr edit` rows | docs/contract.md:732, 733, 734, 735 | the table lists each command twice, the first pair with `--no-needs` and `--title`, the second without | internal: two versions of one command | stale | delete the pair without `--no-needs` (guess: older; unverified, check against commands.md) |
| archive / Archived | docs/contract.md:5, 115, 525 | docs/archive (cpd 1 history), `archive/contexts` (old git store), Archived (cockpit hide, `archived_at`) | internal | loose | no change |
| step | docs/contract.md:369, 639, 431 | engine step (E1..E18), numbered steps of a command, the template step | internal: CONTEXT.md also has Grounding step and PR step | loose | 'engine step E18' with the prefix; no PR step is named in this part |
| run | docs/contract.md:604, 142, 48 | GitHub Actions run, `cpd resource run`, pi run, `run/` directory | upstream: GitHub, pi | loose | no change |
| Fine (summary line) | docs/contract.md:1-748 | Context, Context record, Plan review, Job, Job kind, Open Job, Update Job, Spec, Review Job, Dependency, Attempt, Escalation, Event, Inbox item, Feedback item, PR post, PR poll, Lead Branch, Lead worktree, Gate, Quick check, Tier, Locked, Merged, Manager, Lead, Worker, User, Ledger, Policy key, Resource, Slot, Tenant are each used in one sense in this part (unverified against CONTEXT.md, which was not read in full) | none found | ok | none |

Names for the same thing (section 2.2 objects), as the part uses them:

| Object | The word that names it | Other words that name it here |
|---|---|---|
| Context (:231) | Context | 'the knowledge several Plans share' (:233); 'Epic' in 'Jira Epic' (:233) |
| Plan (:265) | Plan | 'one deliverable' (:3, :267); cpd 1's Workflow (:210, :267); 'Jira task' (:267) |
| PR (:281) | PR (its key: PR key) | 'the part of a Plan in one repo' (:283); also 'cpd PR' (:295), 'adopted PR' (:308), 'base PR' (:310); 'its pull request' names the other thing (:306) |
| Pull request (:316) | Pull request (row of `pull_requests`) | also 'PR' in 'the PR is ready to merge' (:340-343) and 'A PR is ready to post' (:371) |
| Job (:388) | Job | 'self-contained unit of work' (:390) |
| Job kinds (:424) | kind | 'label' (:426); 'Open Job' and 'update Job' (:443, :444) |
| Dependency (:447) | Dependency | field name `needs` (:449); the PR field `needs` (:292) is a second use |
| Attempt (:451) | Attempt | 'one execution of a Job' (:453); 'Worker session' (:453) |
| Session (:488) | Session | 'A pi session' (:490); 'Worker session' (:453); tmux session (:32) |
| Event (:556) | Event | 'record' (:558) |
| Inbox item (:569) | Inbox item | 'needs you' (:571); 'the Inbox row' (:371) |
| Feedback item (:347) | Feedback item | 'An item' (:349) |
| PR post (:365) | PR post | 'post' (:367); 'board post' is another thing (:535) |

## PR occurrences

Method: every occurrence of the word `PR` or `PRs` (335) was classified from its sentence; the counts are by group of lines. Each row's first column gives the section and the line of the first occurrence in that group and sense; the quote is found at that line. The mechanical `object` rewrite is a rule (see Fix list), so `object` rows give counts only.

### A. The word PR / PRs, by section

| Where (section, path:line range) | Count | Sense | Quote (first, at path:line) | After the rename |
|---|---|---|---|---|
| Header and Contents, docs/contract.md:1-22; first at :3 | 2 | object | `one deliverable over one or more PRs (R34). Vocabulary: [CONTEXT.md](../CONTEXT.md). Command-by-command reference (synopses,` | Workflow / Workflows |
| 1.1 What cpd 2 is, docs/contract.md:23-39; first at :36 | 4 | object | `is one deliverable over one or more PRs** (R34): each PR is the Plan's` | Workflow / Workflows |
| 1.2 Output (exit-code tables), docs/contract.md:40-91; first at :85 | 4 | object | `` edit --repo` or `--base` of an adopted PR, §3.8), `context_exists`, `question_answered`, `supersedes_invalid` (§4.6 "A `` | Workflow / Workflows |
| 1.2 Output (exit-code tables), docs/contract.md:40-91; first at :77 | 1 | pull request | `time from the end of one PR poll to the next (§3.7, §8.2)` | unchanged (PR keeps this meaning) |
| 1.3 Conventions, docs/contract.md:92-100; first at :95 | 3 | object | `` - **Selecting a PR** (R34). `--pr <key>`. It may be omitted when the `` | Workflow / Workflows |
| 1.4 $CPD_HOME, docs/contract.md:101-200; first at :136 | 6 | object | `` before later opens of the same PR rewritten as `{"pid", "at"}` when old; `` | Workflow / Workflows |
| 1.4 $CPD_HOME, docs/contract.md:101-200; first at :142 | 2 | pull request | `` the User's approval as the last PR poll read them, `{"version": 1, "polled_at": `` | unchanged (PR keeps this meaning) |
| 1.4 $CPD_HOME, docs/contract.md:101-200; first at :199 | 1 | mixed | `` `pr push` and the clean-up after a PR's merge take no lock shared by `` | see section B |
| 2.1 Identifiers, docs/contract.md:201-226; first at :210 | 12 | object | `` the machine; `pr add` and every PR start (§3.8, A4) refuse when the `` | Workflow / Workflows |
| 2.2 Context, Context record, docs/contract.md:227-264; first at :241 | 4 | object | `its policy key (§8.3, hs-d288): every PR of its Plans is in a` | Workflow / Workflows |
| 2.2 Plan, docs/contract.md:265-280; first at :267 | 3 | object | `One deliverable over one or more PRs (R34), optionally a Jira task. This` | Workflow / Workflows |
| 2.2 Plan, docs/contract.md:265-280; first at :267 | 1 | mixed | `one repo, one Lead Branch and one PR; the per-repo fields moved to the` | see section B |
| 2.2 PR (object), docs/contract.md:281-315; first at :281 | 84 | object | `#### PR` | Workflow / Workflows |
| 2.2 PR (object), docs/contract.md:281-315; first at :298 | 1 | mixed | `` 8b); `pr describe` on an open PR also updates it through GitHub with `` | see section B |
| 2.2 Pull request, docs/contract.md:316-346; first at :318 | 6 | object | `the Dispatch column (commands.md §13.2). A PR (above) points to its row; the` | Workflow / Workflows |
| 2.2 Pull request, docs/contract.md:316-346; first at :318 | 6 | pull request | `What the PR poller (hs-d65, hs-d72, hs-d96, hs-d97, hs-d98, hs-d107, §7.6) last read` | unchanged (PR keeps this meaning) |
| 2.2 Pull request, docs/contract.md:316-346; first at :343 | 4 | mixed | `` request's `base_ref` equals its `default_branch` (the PR is not stacked: a stacked PR `` | see section B |
| 2.2 PR post, docs/contract.md:365-372; first at :369 | 1 | object | `the PR is unstacked by the base PR's merge (§3.7, §7.6 "The base follow").` | Workflow / Workflows |
| 2.2 PR post, docs/contract.md:365-372; first at :365 | 12 | pull request | `#### PR post (pro-d64, pro-d65, pro-d117; Plan pr-post)` | unchanged (PR keeps this meaning) |
| 2.2 PR post, docs/contract.md:365-372; first at :367 | 5 | mixed | `PR for review in Slack. A PR has at most one post (pro-d65):` | see section B |
| 2.2 PR post, docs/contract.md:365-372; first at :367 | 1 | other | `` merge, are in their own sections (PR `flow`, `react`: they are not documented here). `` | see section B |
| 2.2 Plan review, docs/contract.md:373-387; first at :377 | 2 | object | `` (all of them are drafts) and the PRs, and its approval is what `plan `` | Workflow / Workflows |
| 2.2 Job and Spec, docs/contract.md:388-423; first at :390 | 16 | object | `**Locked fields**. A Job runs in its PR's repo: its Attempts' worktrees are cut` | Workflow / Workflows |
| 2.2 Job and Spec, docs/contract.md:388-423; first at :420 | 1 | pull request | `for it); Jobs appended later (CI or PR-feedback fixes) do not become its Dependencies.` | unchanged (PR keeps this meaning) |
| 2.2 Job and Spec, docs/contract.md:388-423; first at :420 | 1 | mixed | `from being Ready (§3.2), so the PR cannot open before the Lead has` | see section B |
| 2.2 Job kinds, docs/contract.md:424-446; first at :426 | 40 | object | `above): the part it plays in its PR's template. A kind is a label` | Workflow / Workflows |
| 2.2 Job kinds, docs/contract.md:424-446; first at :436 | 2 | pull request | `cpd only (the PR poll, E5, §3.7)` | unchanged (PR keeps this meaning) |
| 2.2 Job kinds, docs/contract.md:424-446; first at :443 | 2 | mixed | `` 24); title `Open the pull request: <PR title>`; Tier `open_tier` (§8.2, defaults to the `` | see section B |
| 2.2 Dependency, Attempt, docs/contract.md:447-474; first at :449 | 4 | object | `` in the same Plan **and the same PR** (R34, A7: `needs_other_pr`, exit 3); order `` | Workflow / Workflows |
| 2.2 Asleep, Archived, Deleted, docs/contract.md:513-555; first at :517 | 14 | object | `` a Lead of a Plan: every PR is `merged` or `cancelled`, no Job `` | Workflow / Workflows |
| 2.2 Asleep, Archived, Deleted, docs/contract.md:513-555; first at :529 | 1 | mixed | `` a Plan in `deploy` is archivable, and the engine keeps polling its PR. `` | see section B |
| 2.2 Inbox item, docs/contract.md:569-657; first at :610 | 3 | object | `without the owner. When a Plan's PR points to the row, the item` | Workflow / Workflows |
| 2.2 Inbox item, docs/contract.md:569-657; first at :610 | 7 | pull request | `` `merge_state` is only read here; the PR poll's mergeable state defines it (§2.2 `` | unchanged (PR keeps this meaning) |
| 2.2 Incident, docs/contract.md:658-681; first at :670 | 1 | object | `` the PR key (format `<plan>/<key>`) that triggers auto-end when merged (nullable); replaced by `` | Workflow / Workflows |
| 2.3 Fingerprints, docs/contract.md:682-689; first at :685 | 8 | object | `` `{key, repo, base, needs}` of every PR that is not `cancelled`, with `shape` `` | Workflow / Workflows |
| 2.4 Relations, docs/contract.md:690-724; first at :699 | 28 | object | `PR : "delivered by"` | Workflow / Workflows |
| 2.5 PRs in commands and views, docs/contract.md:725-748; first at :725 | 40 | object | `### 2.5 PRs in commands and views (R34)` | Workflow / Workflows |
| 2.5 PRs in commands and views, docs/contract.md:725-748; first at :743 | 1 | pull request | `the PR poller (hs-d96): one poll of every open pull request of the` | unchanged (PR keeps this meaning) |
| 2.5 PRs in commands and views, docs/contract.md:725-748; first at :742 | 1 | mixed | `` open` is the **repair** since hs-d340: the PR's `open` Job opens it (§7.7) `` | see section B |

### B. The word PR used in a `mixed` or `other` sense (every one listed)

| Where (path:line) | Count | Sense | Quote | After the rename (why mixed, proposal) |
|---|---|---|---|---|
| docs/contract.md:199 | 1 | mixed | `` `pr push` and the clean-up after a PR's merge take no lock shared by `` | 'merge' of the pull request after which cpd cleans up the Workflow's branch and worktree: acts on both. Proposal: write 'the squash merge of the pull request' and 'the Workflow's branch and worktree' |
| docs/contract.md:267 | 1 | mixed | `one repo, one Lead Branch and one PR; the per-repo fields moved to the` | before R34 the single PR was both the Plan's repo part and its pull request. Proposal: 'one Workflow and one pull request' |
| docs/contract.md:298 | 1 | mixed | `` 8b); `pr describe` on an open PR also updates it through GitHub with `` | `pr describe` edits the Workflow's description and the pull request body through gh. Proposal: 'a Workflow whose pull request is open'; `describe` then edits the pull request body through gh |
| docs/contract.md:343 | 1 | mixed | `` request's `base_ref` equals its `default_branch` (the PR is not stacked: a stacked PR `` | 'stacked' is derived from the Workflow's base; the test is on the pull request's base_ref and default_branch. Proposal: state the two facts apart: 'the Workflow is not stacked' (its base) and 'the pull request is ready to merge' |
| docs/contract.md:343 | 1 | mixed | `PR is not stacked: a stacked PR targets another PR's Lead Branch, not` | same: stacked Workflow whose pull request targets another Lead Branch. Proposal: state the two facts apart: 'the Workflow is not stacked' (its base) and 'the pull request is ready to merge' |
| docs/contract.md:343 | 1 | mixed | `not stacked: a stacked PR targets another PR's Lead Branch, not the repo's default).` | same. Proposal: state the two facts apart: 'the Workflow is not stacked' (its base) and 'the pull request is ready to merge' |
| docs/contract.md:345 | 1 | mixed | `never deletes a row: when a PR leaves searches and no PR points` | the pull request leaves the GitHub searches; the object does not. Proposal: 'when the pull request leaves the searches and no Workflow points to its row' |
| docs/contract.md:367 | 1 | mixed | `PR for review in Slack. A PR has at most one post (pro-d65):` | a post announces a pull request but `pr_posts` is keyed (Plan, PR key) (line 616). Proposal: a post announces a pull request but is keyed by (Plan, Workflow key): say 'A pull request has at most one post' and key it by the Workflow |
| docs/contract.md:367 | 1 | mixed | `` on a second run for the same PR. A post is recorded in `pr_posts` `` | same. Proposal: a post announces a pull request but is keyed by (Plan, Workflow key): say 'A pull request has at most one post' and key it by the Workflow |
| docs/contract.md:367 | 1 | other | `` merge, are in their own sections (PR `flow`, `react`: they are not documented here). `` | a section/Plan name "PR flow" that the text says is not documented here (dangling reference) |
| docs/contract.md:369 | 1 | mixed | `` (nothing sent to GitHub) if the PR is **stacked** (its `base` is another `` | stacked is the object's base; auto-merge is enabled on the pull request. Proposal: 'the Workflow is stacked/unstacked'; the auto-merge request is on its pull request |
| docs/contract.md:369 | 1 | mixed | `E18 retries the request once the PR is unstacked by the base PR's` | same. Proposal: 'the Workflow is stacked/unstacked'; the auto-merge request is on its pull request |
| docs/contract.md:369 | 1 | mixed | `on merge never has a stacked PR repointed by GitHub, so its auto-merge` | GitHub repoints the pull request of a stacked Workflow. Proposal: 'the Workflow is stacked/unstacked'; the auto-merge request is on its pull request |
| docs/contract.md:420 | 1 | mixed | `from being Ready (§3.2), so the PR cannot open before the Lead has` | 'open' is the Workflow status and also opening the pull request (open Job). Proposal: 'so the pull request cannot be opened before the Lead has read the findings' |
| docs/contract.md:443 | 1 | mixed | `` 24); title `Open the pull request: <PR title>`; Tier `open_tier` (§8.2, defaults to the `` | the Workflow's `title` given to GitHub as the pull request's title. Proposal: title of the Open Job: `Open the pull request: <Workflow title>` |
| docs/contract.md:444 | 1 | mixed | `` the pull request up to date: <PR title>`, also `open_tier`. **One cause, the conflict**: `` | same. Proposal: title of the Update Job: `Bring the pull request up to date: <Workflow title>` |
| docs/contract.md:529 | 1 | mixed | `` a Plan in `deploy` is archivable, and the engine keeps polling its PR. `` | the engine polls the pull request; 'its PR' is the Workflow's. Proposal: 'the engine keeps polling its pull request' |
| docs/contract.md:742 | 1 | mixed | `` open` is the **repair** since hs-d340: the PR's `open` Job opens it (§7.7) `` | the open Job opens the pull request of the Workflow ('opens it'). Proposal: 'the Workflow's `open` Job opens its pull request' |

### C. Identifiers that contain pr (CLI, flags, codes, tables, files, Events), by token

Counts are occurrences of the token in lines 1-748; `Lines` shows up to 8 lines. `mixed` tokens are listed one occurrence per row in section D.

| Where (path:first line) | Count | Sense | Quote (first, at path:line) | After the rename |
|---|---|---|---|---|
| identifier `pr add`, docs/contract.md:85 (lines 85,95,210,213,276,289,290,291…) | 24 | object | `` `adopt_fork`, `adopt_cpd_branch`, `adopt_taken` and `adopt_foreign` (`pr add --adopt`, pro-n4, pro-n8, §3.8) and `pr_adopted` `` | `workflow add` |
| identifier `<pr>`, docs/contract.md:442 (lines 442,443,444) | 15 | object | `` --tier --spec`: shape `tdd`, the default: `<pr>-docs` (`docs`), `<pr>-tests` (`test`, needs `<pr>-docs`), `<pr>-impl` `` | `<workflow>` |
| identifier `--pr`, docs/contract.md:95 (lines 95,298,371,395,443,741,742,743) | 12 | object | `` - **Selecting a PR** (R34). `--pr <key>`. It may be omitted when the `` | `--workflow` |
| identifier `pr edit`, docs/contract.md:85 (lines 85,95,289,294,297,298,299,308…) | 12 | object | `` pro-n4, pro-n8, §3.8) and `pr_adopted` (`pr edit --repo` or `--base` of an adopted `` | `workflow edit` |
| identifier `prs`, docs/contract.md:276 (lines 276,308,322,369,442,614,685) | 12 | object | `prs` | `workflows` |
| identifier `pr`, docs/contract.md:390 (lines 390,395,685,687) | 5 | object | `` and optionally `resources` (below); with `title`, `tier`, `pr`, `review`, `kind` and the Job's Dependencies, `` | `workflow` (Job field) |
| identifier `pr cancel`, docs/contract.md:95 (lines 95,442,443,736) | 5 | object | `` (§7.6, hs-d96); `pr add`, `pr edit`, `pr cancel` and `pr start` take the key `` | `workflow cancel` |
| identifier `pr_in_use`, docs/contract.md:85 (lines 85,733,735,736) | 5 | object | `` §7.6), `session_ambiguous` (hs-a3, commands §1.3), `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed` (§7.6), `` | `workflow_in_use` |
| identifier `until_pr`, docs/contract.md:669 (lines 669,670,680) | 4 | object | `` the Plan id of the `until_pr`'s Plan (nullable); replaced by a new `--until-pr`, `` | `until_workflow` |
| identifier `base_pr_unavailable`, docs/contract.md:85 (lines 85,314) | 3 | object | `` exists), `needs_other_pr`, `branch_exists` (R34, §3.8), `base_pr_unknown`, `base_pr_unavailable`, `base_pr_other_repo`, `pr_other_plan` (hs-d522, §2.2) and `stacked_rebase` `` | `base_workflow_unavailable` |
| identifier `pr start`, docs/contract.md:95 (lines 95,303,737) | 3 | object | `` add`, `pr edit`, `pr cancel` and `pr start` take the key as their argument. `` | `workflow start` |
| identifier `pr_adopted`, docs/contract.md:85 (lines 85,298,308) | 3 | object | `` add --adopt`, pro-n4, pro-n8, §3.8) and `pr_adopted` (`pr edit --repo` or `--base` of `` | `workflow_adopted` |
| identifier `pr_locked`, docs/contract.md:85 (lines 85,733,735) | 3 | object | `` (hs-a3, commands §1.3), `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed` (§7.6), `pr_title_required` (hs-d285, `` | `workflow_locked` |
| identifier `pr_other_plan`, docs/contract.md:85 (lines 85,312,314) | 3 | object | `` `branch_exists` (R34, §3.8), `base_pr_unknown`, `base_pr_unavailable`, `base_pr_other_repo`, `pr_other_plan` (hs-d522, §2.2) and `stacked_rebase` (hs-d325, §7.6), `` | `workflow_other_plan` |
| identifier `pr_required`, docs/contract.md:84 (lines 84,95) | 3 | object | `` "Tier rules"; hs-d506), `nothing_to_edit`, `file_unreadable`, `invalid_name`, `pr_required`, `invalid_pr_id` (R34), `outline_invalid` (`plan outline`, hs-d502, `` | `workflow_required` |
| identifier `--until-pr`, docs/contract.md:669 (lines 669,670) | 2 | object | `` Plan (nullable); replaced by a new `--until-pr`, or cleared when absent and the `` | `--until-workflow` |
| identifier `base_pr_unknown`, docs/contract.md:85 (lines 85,314) | 2 | object | `` longer exists), `needs_other_pr`, `branch_exists` (R34, §3.8), `base_pr_unknown`, `base_pr_unavailable`, `base_pr_other_repo`, `pr_other_plan` (hs-d522, §2.2) and `` | `base_workflow_unknown` |
| identifier `invalid_pr_id`, docs/contract.md:84 (lines 84,213) | 2 | object | `` rules"; hs-d506), `nothing_to_edit`, `file_unreadable`, `invalid_name`, `pr_required`, `invalid_pr_id` (R34), `outline_invalid` (`plan outline`, hs-d502, commands.md `` | `invalid_workflow_id` |
| identifier `needs_other_pr`, docs/contract.md:85 (lines 85,449) | 2 | object | `` hs-d329's `base_behind`, which no longer exists), `needs_other_pr`, `branch_exists` (R34, §3.8), `base_pr_unknown`, `base_pr_unavailable`, `base_pr_other_repo`, `` | `needs_other_workflow` |
| identifier `pr describe`, docs/contract.md:298 (lines 298) | 2 | object | `` written by the Lead with `cpd pr describe --pr <key> --file <path>`. The `` | `workflow describe` |
| identifier `pr_empty`, docs/contract.md:85 (lines 85,719) | 2 | object | `` `session_ambiguous` (hs-a3, commands §1.3), `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed` (§7.6), `pr_title_required` `` | `workflow_empty` |
| identifier `pr_ended`, docs/contract.md:85 (lines 85,741) | 2 | object | `` §1.3), `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed` (§7.6), `pr_title_required` (hs-d285, §7.6), `description_required` `` | `workflow_ended` |
| identifier `pr_not_found`, docs/contract.md:86 (lines 86,95) | 2 | object | `` branch missing from the remote, §7.6), `pr_not_found` (R34), `topic_not_found` (`context show --topic`, hs-d549, `` | `workflow_not_found` |
| identifier `pr_title_required`, docs/contract.md:85 (lines 85,297) | 2 | object | `` `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed` (§7.6), `pr_title_required` (hs-d285, §7.6), `description_required` and `description_invalid` (commands.md `` | `workflow_title_required` |
| identifier `PR_NEED`, docs/contract.md:700 (lines 700) | 1 | object | `PR_NEED : "needs (a PR of the same Plan)"` | WORKFLOW_NEED |
| identifier `base_pr`, docs/contract.md:310 (lines 310) | 1 | object | `` and `pr view` print it as `base_pr` (the key, or null), and no `` | `base_workflow` |
| identifier `policy_of_pr`, docs/contract.md:617 (lines 617) | 1 | object | `` **POST ENABLED:** the owner's resolved policy (`pr::policy_of_pr`) has `pr.post.enabled` true. An unowned pull `` | `policy_of_workflow` |
| identifier `pr-adopt`, docs/contract.md:295 (lines 295) | 1 | object | `` stored value for new PRs, or `cpd/<plan>/<key>` for legacy ones); migration `pr-adopt` (§4.5) `` | applied migration name: history keeps the old word |
| identifier `pr-title`, docs/contract.md:297 (lines 297) | 1 | object | `` a PR that existed before migration `pr-title` (§4.5); `pr open` refuses such a `` | applied migration name: history keeps the old word |
| identifier `pr_needs_no_delete`, docs/contract.md:538 (lines 538) | 1 | object | `` refuses (`T_no_delete`, and the hand-written `events_no_delete`, `pr_needs_no_delete` and `dependencies_no_delete`). Each gains **one exception `` | `workflow_needs_no_delete` |
| identifier `pr_start_failed`, docs/contract.md:589 (lines 589) | 1 | object | `` the errors addressed to a Lead (`pr_start_failed`, `push_failed`, driver errors), nor the engine's `` | `workflow_start_failed` |
| identifier `pr-post`, docs/contract.md:365 (lines 365) | 1 | other | `#### PR post (pro-d64, pro-d65, pro-d117; Plan pr-post)` | a Plan name (history); no change |
| identifier `pr`, docs/contract.md:593 (lines 593) | 1 | phase | `` when its Plan is in `dispatch` or `pr`; magenta in `grounding`, `plan`, `done` or `` | the phase is `deploy` (stale) |
| identifier `pr-ready`, docs/contract.md:343 (lines 343,571,602,610,612,635) | 10 | pull request | `` of this predicate (§2.2 Inbox item `pr-ready` rule, E8 reconcile, the Deploy column's `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pull_requests`, docs/contract.md:318 (lines 318,363,548,602,610,612,649) | 7 | pull request | `` a row and no PR. Table `pull_requests`, migration 6; `mergeable` and `merge_state` come `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr poll`, docs/contract.md:59 (lines 59,61,95,123,304,743) | 6 | pull request | `` `store_busy`, `lock_timeout` (`pr poll`'s `locks/pr-poll.lock`, §7.6; no git operation waits for a cpd `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr post`, docs/contract.md:83 (lines 83,85,367,371) | 5 | pull request | `` refusal and, the same, E11's and `pr post`'s `error` Event's code; commands.md §21.5, §21.6, `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr-feedback`, docs/contract.md:345 (lines 345,349,610,635) | 4 | pull request | `` no Event of its own (§5.1); `pr-feedback` and `pr-merged` come from the transitions `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr merge`, docs/contract.md:136 (lines 136,349,610) | 3 | pull request | `` the marker's lifecycle: at start before `cpd pr merge`, rewritten with `"merged": true` on `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr reply`, docs/contract.md:85 (lines 85,349,351) | 3 | pull request | `` `pr_not_closed` (§7.6, §3.8), `pr_not_open` and `feedback_not_found` (`pr reply`, §7.6; pro-d68), `base_conflict` (§7.6, hs-d490: it `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr-base`, docs/contract.md:340 (lines 340,341,343) | 3 | pull request | `` the poll; null until the migration `pr-base` (§4.5). Used to determine if the `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pull_request`, docs/contract.md:301 (lines 301,322,614) | 3 | pull request | `` `pull_request` `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr`, docs/contract.md:300 (lines 300,306) | 2 | pull request | `` `pr` `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr-changes`, docs/contract.md:601 (lines 601,610) | 2 | pull request | `` `pr-changes` `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr-comments`, docs/contract.md:337 (lines 337,338) | 2 | pull request | `` of its feedback (§7.6 step 3a; pro-d68); NULL = never read. Migration `pr-comments` `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr-failing`, docs/contract.md:600 (lines 600,610) | 2 | pull request | `` `pr-failing` `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr-poll`, docs/contract.md:59 (lines 59,123) | 2 | pull request | `` `store_busy`, `lock_timeout` (`pr poll`'s `locks/pr-poll.lock`, §7.6; no git operation waits for a cpd `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr_posts`, docs/contract.md:367 (lines 367,616) | 2 | pull request | `` PR. A post is recorded in `pr_posts` with its Slack channel, message timestamp `` | unchanged (names the pull request or something read from GitHub) |
| identifier `close_pr_ready`, docs/contract.md:635 (lines 635) | 1 | pull request | `` (the paths that store a merge, `close_pr_ready`, still close it in the same `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr-merged`, docs/contract.md:345 (lines 345) | 1 | pull request | `` of its own (§5.1); `pr-feedback` and `pr-merged` come from the transitions it causes `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr-poll-failing`, docs/contract.md:133 (lines 133) | 1 | pull request | `` pr-poll-failing.json          `{call, since}`: the engine's record of when E5's `gh` call first failed `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr.post.enabled`, docs/contract.md:617 (lines 617) | 1 | pull request | `` the owner's resolved policy (`pr::policy_of_pr`) has `pr.post.enabled` true. An unowned pull request, or `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr_not_open`, docs/contract.md:85 (lines 85) | 1 | pull request | `` commands.md §9.1, §7.6), `pr_not_closed` (§7.6, §3.8), `pr_not_open` and `feedback_not_found` (`pr reply`, §7.6; pro-d68), `` | unchanged (names the pull request or something read from GitHub) |
| identifier `pr_poll_seconds`, docs/contract.md:77 (lines 77) | 1 | pull request | `` E5's `watch.pr_poll_seconds`, the time from the end of one PR poll to the `` | unchanged (names the pull request or something read from GitHub) |

### D. Identifiers in a `mixed` sense (every occurrence listed)

| docs/contract.md:95 | 1 | mixed | `` (exit 4). Used by `job add`, `pr open`, `pr rebase` and `pr view` (§2.5); `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:157 | 1 | mixed | `` of the last Gate run by `pr open`, `pr rebase` or `pr push` on `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:199 | 1 | mixed | `` is **no per-repo lock** (hs-d243): merges, `pr open`, `pr rebase`, `pr push` and the `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:290 | 1 | mixed | `` reader of `base` (§7.2 start, §7.6 `pr open`, the Gate's `target`) reads the branch `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:297 | 1 | mixed | `` that existed before migration `pr-title` (§4.5); `pr open` refuses such a PR (`pr_title_required`, §7.6). `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:297 | 1 | mixed | `` refuses such a PR (`pr_title_required`, §7.6). `pr open` gives it to GitHub as the `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:298 | 1 | mixed | `` made before the description was required. `pr open` refuses such a PR (`description_required`, §7.6). `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:298 | 1 | mixed | `` refuses such a PR (`description_required`, §7.6). `pr open` gives it to GitHub as the `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:299 | 1 | mixed | `` fills it: one ticket per PR. `pr open` and the open Job's merge refuse `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:300 | 1 | mixed | `` Job's merge, §7.7, or the repair `pr open`); `opened_at` is when the PR became `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:301 | 1 | mixed | `` hs-d96, migration 6, §4.5): set by `pr open`, non-null exactly when the status is `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:331 | 1 | mixed | `` included: GitHub computes it lazily after the base moves. `pr open` inserts `unknown` `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:332 | 1 | mixed | `` `mergeStateStatus`, lowercased (§7.6). Stored as each poll reads it. `pr open` inserts `unknown` `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:443 | 1 | mixed | `` stands, the way on is `pr open --pr` or `pr cancel`, §3.8). Engine `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:610 | 1 | mixed | `` the poll's search or lookup, or `pr open`'s adoption (hs-d526, hs-n404; hs-d550 B3): the `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:742 | 1 | mixed | `` `pr open --pr <key>`, `pr rebase --pr <key>`, `pr view --pr <key>` `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:742 | 1 | mixed | `` selected by the rule of §1.3. `pr open` is the **repair** since hs-d340: the `` | `pr open`: acts on a Workflow (its Job opens it) and opens its pull request |
| docs/contract.md:95 | 1 | mixed | `` Used by `job add`, `pr open`, `pr rebase` and `pr view` (§2.5); `pr poll` `` | `pr rebase`: merges the base into the Workflow's Lead worktree and pushes to its pull request |
| docs/contract.md:157 | 1 | mixed | `` last Gate run by `pr open`, `pr rebase` or `pr push` on PR <key> `` | `pr rebase`: merges the base into the Workflow's Lead worktree and pushes to its pull request |
| docs/contract.md:199 | 1 | mixed | `` per-repo lock** (hs-d243): merges, `pr open`, `pr rebase`, `pr push` and the clean-up after `` | `pr rebase`: merges the base into the Workflow's Lead worktree and pushes to its pull request |
| docs/contract.md:444 | 1 | mixed | `` is queued for feedback (§3.8 "Updating"). `pr rebase` and `pr push` remain (§7.6) for `` | `pr rebase`: merges the base into the Workflow's Lead worktree and pushes to its pull request |
| docs/contract.md:742 | 1 | mixed | `` `pr open --pr <key>`, `pr rebase --pr <key>`, `pr view --pr <key>` `` | `pr rebase`: merges the base into the Workflow's Lead worktree and pushes to its pull request |
| docs/contract.md:95 | 1 | mixed | `` add`, `pr open`, `pr rebase` and `pr view` (§2.5); `pr poll` has no `--pr`: `` | `pr view`: shows the Workflow and its pull request together |
| docs/contract.md:310 | 1 | mixed | `` derived, never stored: `plan show` and `pr view` print it as `base_pr` (the key, `` | `pr view`: shows the Workflow and its pull request together |
| docs/contract.md:349 | 1 | mixed | `` poll (§7.6 step 3a), `plan show`, `pr view` and `pr reply` (commands.md §4.3, §9.8, `` | `pr view`: shows the Workflow and its pull request together |
| docs/contract.md:361 | 1 | mixed | `` review body is never an item (`pr view` still lists them). "Later" is by `` | `pr view`: shows the Workflow and its pull request together |
| docs/contract.md:742 | 1 | mixed | `` `pr open --pr <key>`, `pr rebase --pr <key>`, `pr view --pr <key>` `` | `pr view`: shows the Workflow and its pull request together |
| docs/contract.md:157 | 1 | mixed | `` run by `pr open`, `pr rebase` or `pr push` on PR <key> (§7.6) `` | `pr push`: pushes the Workflow's Lead Branch, which updates its pull request |
| docs/contract.md:199 | 1 | mixed | `` (hs-d243): merges, `pr open`, `pr rebase`, `pr push` and the clean-up after a PR's `` | `pr push`: pushes the Workflow's Lead Branch, which updates its pull request |
| docs/contract.md:444 | 1 | mixed | `` feedback (§3.8 "Updating"). `pr rebase` and `pr push` remain (§7.6) for what a Job `` | `pr push`: pushes the Workflow's Lead Branch, which updates its pull request |
| docs/contract.md:85 | 1 | mixed | `` commands §1.3), `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed` (§7.6), `pr_title_required` (hs-d285, §7.6), `` | `pr_not_ready`: refusal of `pr start` (needs not merged: Workflow) at :737 and the reasons of `pr post` readiness (pull request) at :371 |
| docs/contract.md:371 | 1 | mixed | `` The reasons match the error code `pr_not_ready` reasons (§1.2). This definition is also `` | `pr_not_ready`: refusal of `pr start` (needs not merged: Workflow) at :737 and the reasons of `pr post` readiness (pull request) at :371 |
| docs/contract.md:737 | 1 | mixed | `` PR whose needs are all `merged` (`pr_not_ready`, exit 3): the retry after a `` | `pr_not_ready`: refusal of `pr start` (needs not merged: Workflow) at :737 and the reasons of `pr post` readiness (pull request) at :371 |
| docs/contract.md:85 | 1 | mixed | `` §7.6), `jira_required` (pro-d127, commands.md §9.1, §7.6), `pr_not_closed` (§7.6, §3.8), `pr_not_open` and `feedback_not_found` (`pr `` | `pr_not_closed`: `pr cancel` of an open Workflow when its pull request is stored closed: the Workflow's status and the pull request's state together |
| docs/contract.md:736 | 1 | mixed | `` PR's pull request is stored `closed` (`pr_not_closed`, exit 3 otherwise), cancelling its `draft`, `` | `pr_not_closed`: `pr cancel` of an open Workflow when its pull request is stored closed: the Workflow's status and the pull request's state together |
| docs/contract.md:299 | 1 | mixed | `` refuse a PR whose policy sets `pr.jira_required` and whose `jira` is null, unless `` | `pr.jira_required`: policy key on a pull-request-opening rule that reads the Workflow's `jira` |
| docs/contract.md:85 | 1 | mixed | `` `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed` (§7.6), `pr_title_required` (hs-d285, §7.6), `description_required` and `` | `pr_closed`: named only in the list of 1.2; its meaning is in section 7.6 (other part), so the sense cannot be told here |
| docs/contract.md:85 | 1 | mixed | `` (hs-d139, §7.6), `session_ambiguous` (hs-a3, commands §1.3), `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed` `` | `pr_exists`: named only in the list of 1.2; its meaning is in section 7.6 (other part), so the sense cannot be told here |

### E. The words 'pull request' (99), by section

All are the GitHub pull request: `pull request`. None needs a change. Rows give the count and the first quote.

| Where (section, path:line range) | Count | Sense | Quote (first, at path:line) | After the rename |
|---|---|---|---|---|
| Header and Contents, docs/contract.md:1-22; first at :5 | 1 | pull request | `` Branch, Lead worktree, Gate and GitHub pull request; Job `r34-contract`, §10), R35 (§10) `` | unchanged |
| 1.1 What cpd 2 is, docs/contract.md:23-39; first at :29 | 2 | pull request | `` **>= 3.6** (§6.1), `pi` and, for pull requests only, `gh`. No `bd`, no `wt` `` | unchanged |
| 1.3 Conventions, docs/contract.md:92-100; first at :95 | 1 | pull request | `` no `--pr`: it polls every open pull request of the User (§7.6, hs-d96); `pr `` | unchanged |
| 1.4 $CPD_HOME, docs/contract.md:101-200; first at :136 | 3 | pull request | `` a merge in progress for that pull request, `{"pid", "at", "merged"}` (the action's `` | unchanged |
| 2.2 PR (object), docs/contract.md:281-315; first at :283 | 15 | pull request | `its Gate run and one GitHub pull request opened from its Lead Branch. No` | unchanged |
| 2.2 Pull request, docs/contract.md:316-346; first at :316 | 16 | pull request | `#### Pull request` | unchanged |
| 2.2 Feedback item, docs/contract.md:347-364; first at :349 | 2 | pull request | `thing a person said on a pull request that a Plan owns. **This subsection` | unchanged |
| 2.2 PR post, docs/contract.md:365-372; first at :371 | 1 | pull request | `` for review` status (§2.2 "The authored pull request rule"), and the cockpit's Inbox menu `` | unchanged |
| 2.2 Job kinds, docs/contract.md:424-446; first at :435 | 6 | pull request | `the Gate, pushes and opens the pull request, ready, never a draft (hs-d491,` | unchanged |
| 2.2 Asleep, Archived, Deleted, docs/contract.md:513-555; first at :535 | 1 | pull request | `files, worktrees and local branches. GitHub pull requests and remote branches are left alone.` | unchanged |
| 2.2 Inbox item, docs/contract.md:569-657; first at :571 | 43 | pull request | `while a dialog is open, a pull request's status changes colour), the item` | unchanged |
| 2.4 Relations, docs/contract.md:690-724; first at :716 | 4 | pull request | `` - A PR points to **0..1 Pull request** (§2.2): none while `waiting` or `` | unchanged |
| 2.5 PRs in commands and views, docs/contract.md:725-748; first at :734 | 4 | pull request | `` [--confirm-author <login>]` instead of `--base`: an existing pull request, §2.2 "An adopted PR") `` | unchanged |

### F. `phase` and `step` uses

| Where (path:line) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| docs/contract.md:593 | 1 | phase | `` red when its Plan is in `dispatch` or `pr`; `` | stale: the phase is `deploy`; write `dispatch` or `deploy` |
| (none) | 0 | step | no use of PR as a step of the Deploy stage in lines 1-748 (`rg -n 'PR step'` gives no line) | none |

## Fix list

Rules first (mechanical, counted; not listed one by one):

| Rule | Applies to | Count |
|---|---|---|
| R1: the word `PR` / `PRs` in sense `object` becomes `Workflow` / `Workflows` | section A rows with sense `object` | 285 (by section: Header and Contents 2; 1.1 What cpd 2 is 4; 1.2 Output (exit-code tables) 4; 1.3 Conventions 3; 1.4 $CPD_HOME 6; 2.1 Identifiers 12; 2.2 Context, Context record 4; 2.2 Plan 3; 2.2 PR (object) 84; 2.2 Pull request 6; 2.2 PR post 1; 2.2 Plan review 2; 2.2 Job and Spec 16; 2.2 Job kinds 40; 2.2 Dependency, Attempt 4; 2.2 Asleep, Archived, Deleted 14; 2.2 Inbox item 3; 2.2 Incident 1; 2.3 Fingerprints 8; 2.4 Relations 28; 2.5 PRs in commands and views 40) |
| R2: identifiers of sense `object` take the noun `workflow` (`cpd pr add` becomes `cpd workflow add`, `--pr` becomes `--workflow`, `prs` becomes `workflows`, codes `pr_*` become `workflow_*`, `base_pr_*` becomes `base_workflow_*`, Job field `pr` becomes `workflow`, `<pr>` becomes `<workflow>`); applied migration names keep the old word | section C rows with sense `object` | 137 |
| R3: `PR` and `pr` identifiers of sense `pull request` stay (`pr poll`, `pr post`, `pr reply`, `pr merge`, `pr_posts`, `pr-feedback`, `pr-merged`, `pr-ready`, `pr-failing`, `pr-changes`, `pr_not_open`, `pull_requests`, `pull_request`) | sections A and C | 94 |
| R4: the headings 'PR' (:281), 'PR post' (:365; stays), 'PRs in commands and views' (:725), the diagram nodes PR/PR_NEED (:699-703) | section 2 | rewrite the first and third and the diagram; keep 'PR post' |

Judgement items (each listed):

| path:line | What to change | Why |
|---|---|---|
| docs/contract.md:32 | 'a hidden session of the same tmux server' -> 'a hidden tmux session' | loose: Session is cpd's object |
| docs/contract.md:142 | JSON key `"workflow"` of `deploy-runs.json` -> a key that cannot be read as a cpd Workflow (for example `actions_workflow`); the User decides | loose: collides with the new Workflow |
| docs/contract.md:210 | '(cpd 1's Workflow id)' and 'cpd 1 Workflows' -> reword to a history note that does not read as the new Workflow | stale: Workflow now means the renamed PR object |
| docs/contract.md:267 | 'This was cpd 1's Workflow (D3).' -> reword or drop (same reason as :210) | stale |
| docs/contract.md:283 | 'No other term names it (hs-d36).' -> delete or reword: the Workflow is the only name; 'PR' is never its name | after the rename 'PR' names a second thing (the pull request) |
| docs/contract.md:299 | 'the PR's own Jira ticket' (and :267 'a Jira task') -> use 'ticket' at :267 | loose: Plan is the Ticket level, Job is the Task level in CONTEXT.md |
| docs/contract.md:300 | Workflow field `pr` (json) beside `pull_request` (:301) -> make one field of them | overloaded: two fields name the pull request; the Job field `pr` (:395) names a Workflow |
| docs/contract.md:306 | 'The PR object is the cpd record; "pull request" names the object on GitHub that its `pr` field points to.' -> delete, or say 'A Workflow maps to one pull request but is not it' | the sentence defines PR as the cpd object, the opposite of the decision (wrong after the rename) |
| docs/contract.md:403 | Spec field `context` -> rename (for example `background`); the User decides | overloaded: the lowercase field and the Context entity differ only by case; touches Spec files |
| docs/contract.md:577 | 'the tier names are the palette's colours' -> 'the urgency names are the palette's colours' | wrong: Tier means a model tier |
| docs/contract.md:593 | `dispatch` or `pr` -> `dispatch` or `deploy` | phase `pr` no longer exists (`phase`; stale); Plan phases are at :274 |
| docs/contract.md:614 | '**OWNER:** the newest Plan …' -> 'owning Plan' | overloaded: 'owner' is a GitHub login at :211 and :241 |
| docs/contract.md:639 | 'A deploy is a workflow run' -> 'A deploy is a GitHub Actions workflow run' | loose: 'workflow' must be written in full for GitHub Actions |
| docs/contract.md:670 | `until_pr` 'the PR key (format `<plan>/<key>`)' -> rename the column `until_workflow`; define or drop the `<plan>/<key>` form | wrong: :213 and :312 say a key has no `/` |
| docs/contract.md:732 | `--base` 'as `<plan>/<key>`, a base PR to stack on' (also :734) -> 'a bare `<key>`' | wrong: :312 says `--base` names a base PR only by its bare key |
| docs/contract.md:734 | second pair of `pr add` / `pr edit` rows (:734, :735) repeats :732, :733 -> delete the pair that lacks `--no-needs` (guess, unverified: the older one) | stale duplicate rows |
| docs/contract.md:199 | `mixed`: `` `pr push` and the clean-up after a PR's merge take no lock shared by `` -> write 'the squash merge of the pull request' and 'the Workflow's branch and worktree' | 'merge' of the pull request after which cpd cleans up the Workflow's branch and worktree: acts on both |
| docs/contract.md:267 | `mixed`: `one repo, one Lead Branch and one PR; the per-repo fields moved to the` -> 'one Workflow and one pull request' | before R34 the single PR was both the Plan's repo part and its pull request |
| docs/contract.md:298 | `mixed`: `` 8b); `pr describe` on an open PR also updates it through GitHub with `` -> 'a Workflow whose pull request is open'; `describe` then edits the pull request body through gh | `pr describe` edits the Workflow's description and the pull request body through gh |
| docs/contract.md:343 | `mixed`: `` request's `base_ref` equals its `default_branch` (the PR is not stacked: a stacked PR `` -> state the two facts apart: 'the Workflow is not stacked' (its base) and 'the pull request is ready to merge' | 'stacked' is derived from the Workflow's base; the test is on the pull request's base_ref and default_branch |
| docs/contract.md:343 | `mixed`: `PR is not stacked: a stacked PR targets another PR's Lead Branch, not` -> state the two facts apart: 'the Workflow is not stacked' (its base) and 'the pull request is ready to merge' | same: stacked Workflow whose pull request targets another Lead Branch |
| docs/contract.md:343 | `mixed`: `not stacked: a stacked PR targets another PR's Lead Branch, not the repo's default).` -> state the two facts apart: 'the Workflow is not stacked' (its base) and 'the pull request is ready to merge' | same |
| docs/contract.md:345 | `mixed`: `never deletes a row: when a PR leaves searches and no PR points` -> 'when the pull request leaves the searches and no Workflow points to its row' | the pull request leaves the GitHub searches; the object does not |
| docs/contract.md:367 | `mixed`: `PR for review in Slack. A PR has at most one post (pro-d65):` -> a post announces a pull request but is keyed by (Plan, Workflow key): say 'A pull request has at most one post' and key it by the Workflow | a post announces a pull request but `pr_posts` is keyed (Plan, PR key) (line 616) |
| docs/contract.md:367 | `mixed`: `` on a second run for the same PR. A post is recorded in `pr_posts` `` -> a post announces a pull request but is keyed by (Plan, Workflow key): say 'A pull request has at most one post' and key it by the Workflow | same |
| docs/contract.md:367 | `other`: `` merge, are in their own sections (PR `flow`, `react`: they are not documented here). `` -> name the section/Plan that holds 'flow' and 'react', or delete the reference | a dangling pointer: the text says the topic 'is not documented here' |
| docs/contract.md:369 | `mixed`: `` (nothing sent to GitHub) if the PR is **stacked** (its `base` is another `` -> 'the Workflow is stacked/unstacked'; the auto-merge request is on its pull request | stacked is the object's base; auto-merge is enabled on the pull request |
| docs/contract.md:369 | `mixed`: `E18 retries the request once the PR is unstacked by the base PR's` -> 'the Workflow is stacked/unstacked'; the auto-merge request is on its pull request | same |
| docs/contract.md:369 | `mixed`: `on merge never has a stacked PR repointed by GitHub, so its auto-merge` -> 'the Workflow is stacked/unstacked'; the auto-merge request is on its pull request | GitHub repoints the pull request of a stacked Workflow |
| docs/contract.md:420 | `mixed`: `from being Ready (§3.2), so the PR cannot open before the Lead has` -> 'so the pull request cannot be opened before the Lead has read the findings' | 'open' is the Workflow status and also opening the pull request (open Job) |
| docs/contract.md:443 | `mixed`: `` 24); title `Open the pull request: <PR title>`; Tier `open_tier` (§8.2, defaults to the `` -> title of the Open Job: `Open the pull request: <Workflow title>` | the Workflow's `title` given to GitHub as the pull request's title |
| docs/contract.md:444 | `mixed`: `` the pull request up to date: <PR title>`, also `open_tier`. **One cause, the conflict**: `` -> title of the Update Job: `Bring the pull request up to date: <Workflow title>` | same |
| docs/contract.md:529 | `mixed`: `` a Plan in `deploy` is archivable, and the engine keeps polling its PR. `` -> 'the engine keeps polling its pull request' | the engine polls the pull request; 'its PR' is the Workflow's |
| docs/contract.md:742 | `mixed`: `` open` is the **repair** since hs-d340: the PR's `open` Job opens it (§7.7) `` -> 'the Workflow's `open` Job opens its pull request' | the open Job opens the pull request of the Workflow ('opens it') |
| docs/contract.md:95, 157, 199, 290, 297, 298, 299, 300, 301, 310, 331, 332, 349, 361, 443, 444, 610, 742 (every `pr open` / `pr rebase` / `pr push` / `pr view`, section D) | decide the noun of each command that touches both a Workflow and its pull request | `mixed`: the User decides; proposal: keep `pr` for commands that talk to GitHub (`poll`, `post`, `reply`, `merge`) and use `workflow` for the others; `open`, `push`, `rebase`, `view` straddle both, so their noun is the User's call |
| docs/contract.md:85, 371, 737 | `pr_not_ready` names two refusals (a Workflow whose needs are not merged; a pull request that is not ready to post) | `mixed`: split into two codes or rename one |
| docs/contract.md:85 | `pr_closed`, `pr_exists` : sense not decidable from this part | `mixed`: check section 7.6 (other audit Job) before renaming |

## Counts

- Files read in full: 1 (docs/contract.md lines 1-748); CONTEXT.md lines 1-35 read as a reference only.
- Lines read: 748 of docs/contract.md (+35 of CONTEXT.md).
- Rows: Terms 31 (30 term rows and 1 summary line) plus the 13-row names table; PR occurrences A 39, B 18, C 54, D 38, E 13, F 2; Fix list 37 judgement rows and 4 rule rows.
- Occurrences of the word `PR` / `PRs`: 335: object 285, pull request 32, mixed 17, other 1, phase 0, step 0.
- Occurrences of identifiers that contain pr (regex in Coverage): 239: object 137, pull request 62, mixed 38, other 1, phase 1, step 0.
- Occurrences of 'pull request(s)': 99, all sense pull request.
- All three together (word PR, identifiers, 'pull request'): mixed 55, object 422, other 2, phase 1, pull request 193 (total 673).
