# Glossary audit: docs/commands.md section 9, the `pr` command group

Audit of Job `aud-commands-2` (Plan `glassary`, Attempt `aud-commands-2-a1`). It reads, classifies and reports; it changes no file but this one. The decision behind it (Context records pro-d136 and pro-d138): `PR` means only the GitHub pull request; the cpd object that was also called PR is renamed Workflow. Sense codes are the Spec's six: `object` (the cpd object, a Workflow after the rename), `pull request`, `phase`, `step`, `mixed`, `other`. Every proposal below is a proposal: the User decides contested terms.

## Coverage

The part is `docs/commands.md` lines 1731 to 2358 (628 lines): from the heading `## 9. `pr`` (line 1731) through the line before `## 10. `session`` (line 2359). Subsections and line ranges:

| Subsection | Lines |
|---|---|
| 9 (intro) | 1731-1734 |
| 9.1 `pr open` | 1735-1804 |
| 9.2 `pr poll` | 1805-1856 |
| 9.3 `pr rebase` | 1857-1908 |
| `pr rebase --continue` | 1909-1928 |
| `pr rebase --abort` | 1929-1932 |
| 9.4 `pr add` | 1933-2014 |
| 9.5 `pr edit` | 2015-2056 |
| 9.5a `pr describe` | 2057-2080 |
| 9.6 `pr cancel` | 2081-2122 |
| 9.7 `pr start` | 2123-2153 |
| 9.8 `pr view` | 2154-2186 |
| 9.8a `pr reply` | 2187-2218 |
| 9.8b `pr post` | 2219-2269 |
| 9.9 `pr push` | 2270-2305 |
| 9.10 `pr merge` | 2306-2337 |
| 9.11 `host approve` and `host repair` | 2338-2350 |
| 9.12 `cpd tui action host` | 2351-2354 |
| 9.13 `cpd tui action incident` | 2355-2358 |

- **Read in full:** all 628 lines of the part, with the `read` tool in three calls (1731-1996, 1997-2196, 2197-2361). Nothing in the part was only skimmed.
- **Read in full, outside the part:** `CONTEXT.md` (323 lines), as the glossary the terms are checked against.
- **Only searched, not read:** `docs/contract.md`, with `rg -n 'gate\.pr|`gate`' docs/contract.md` and a few `sed -n` reads of single lines (907, 1124, 1133, 1193, 3561, 3963, 2416, 4252) and, in `docs/commands.md`, line 4721, to resolve one reference each. Unverified: anything about contract.md beyond those lines.
- **Not read:** the rest of `docs/commands.md` (other parts of the audit), `docs/contract.md`, `docs/adr`, `docs/archive`, `docs/research` (history keeps the old words, and other parts cover the rest). The cross-references this part makes into sections 4, 5, 13, 21, 29 were not followed.
- **Classification method:** the `PR`/`PRs` hits were dumped with their context (`/tmp` file, 374 lines) and read once; every hit was coded `object` unless it is listed individually under `mixed`, `pull request` or `other` below. Unverified: a second reader may move a hit between `object` and `mixed`. The numbers in `## Counts` come from a script that applies exactly the individually listed overrides to the regex hits (the script asserts that each override matches one hit).

The `rg` commands behind each count (run from the repo root; `P` is `sed -n 1731,2358p docs/commands.md`):

- lines in the part: `P | wc -l` gives 628.
- `PR` or `PRs` tokens: `P | rg -o '\bPRs?\b' | wc -l` gives 415.
- `pull request(s)` tokens: `P | rg -o -i 'pull requests?' | wc -l` gives 94.
- `--pr` flag mentions: `P | rg -o -e '--pr\b' | wc -l` gives 27.
- `prs` (table, columns, keys): `P | rg -o '\bprs\b' | wc -l` gives 19.
- `pull_request` and `pull_requests`: `P | rg -o '\bpull_requests?\b' | wc -l` gives 15.
- Event kinds `pr-started`, `pr-base-changed`, `pr-added`, `pr-edited`, `pr-described`, `pr-cancelled`, `pr-opened`, `pr-merged`, `pr-feedback`, `pr-ready`: `P | rg -o '`pr-(started|base-changed|added|edited|described|cancelled|opened|merged|feedback|ready)`' | wc -l` gives 30.
- the word workflow: `P | rg -o -i '\bworkflows?\b' | wc -l` gives 13.
- old PR phase or step: `P | rg -i -c 'PR phase|PR step|review phase|phase.{0,20}\bPR\b'` gives 7 hits, all false positives (next paragraph).

The 7 hits of the last command are `"phase": ...` JSON next to `"pr": ...`, `no illegal_phase. A new PR`, and "every phase and every PR status" (lines 1794, 1800, 1995, 2026, 2028, 2063, 2089): each puts the Plan phase and a Workflow status side by side; none uses a PR phase. So the `phase` code has 0 uses in this part.

## Terms

One row per term or word-to-watch used in more than one sense, loosely, stale, wrong or colliding. Where cells quote, the quote is at the cited line.

| Term | Where (path:line) | Sense as used here | Collision | Verdict | Proposed wording  |
|---|---|---|---|---|---|
| PR | docs/commands.md:1733 | "A Plan's PRs (R34, §2.2 PR, §3.8) and their pull requests": the object, then the pull request, in one sentence | internal: PR (entity), PR (phase), PR (step) of CONTEXT.md; upstream: GitHub pull request | overloaded | “A Plan's Workflows (R34, §2.2 Workflow, §3.8) and their pull requests”  |
| PR | docs/commands.md:1737 | "A PR is opened by its `open` Job": the Workflow's pull request is opened and its status becomes `open` | internal: PR object vs pull request | loose | “A Workflow's pull request is opened by its `open` Job”  |
| PR | docs/commands.md:1771 | "Opened ready, never a draft, like every PR.": draft and ready belong to the pull request | upstream: GitHub draft | loose | “like every pull request”  |
| step | docs/commands.md:1739 | "`pr open` is the **Mechanical checks** step of the Deploy stage": it runs the Gate and then opens the pull request, so it spans two steps of CONTEXT.md | internal: CONTEXT.md:72 puts "`pr open` is the repair" under the step named PR (the pull request is opened) | loose | “`pr open` repairs the Mechanical checks step and the step that opens the pull request” (the new name of the PR step is the User's)  |
| Deploy | docs/commands.md:1830 | "The deploy read": GitHub Actions deployments waiting for approval, read through `[deploys] workflows` | internal: Deploy stage, Deploy column, Plan phase `deploy`; upstream: GitHub deployment | overloaded | “The pending-deployment read”  |
| Deploy column | docs/commands.md:1831 | "what the Deploy column shows": the cockpit column of the `authored` pull requests | internal: CONTEXT.md:186 says the PR poll "feeds the Dispatch column" | loose | one name for the column in both documents (guess: Deploy, the stage it shows)  |
| workflow | docs/commands.md:1830 | "for each workflow a repo policy file declares in `[deploys] workflows`": a GitHub Actions workflow | internal: Workflow (new meaning); upstream: GitHub Actions workflow | overloaded | “for each GitHub Actions workflow” (the Spec: always written in full); the key name `workflows` is a policy-file identifier, not checked here  |
| workflow worktree | docs/commands.md:2105,2107,2138,2145,2146 | "`lead_worktree` (the workflow worktree)": cpd 1's name for the Lead worktree | internal: Workflow (new meaning) would make it a Workflow's worktree by accident | stale | “Lead worktree” (the part's own term, 1733 `<lead worktree>`)  |
| `workflow init` | docs/commands.md:1946,1954 | "(cpd 1 §11 `workflow init`, kept)": a cpd 1 command cited for the default base | internal: `workflow` becomes a live noun after the rename | loose | “cpd 1's retired `workflow init`” so it cannot be read as the new noun  |
| dispatch | docs/commands.md:1751 | "the PR is `dispatch`": a Workflow status | internal: Plan phase `dispatch` (docs/commands.md:2135 "the phase is `dispatch`"), Dispatch phase, Dispatching step | overloaded | “the Workflow's status is `dispatch`”; a distinct status word is a design decision for the User  |
| open | docs/commands.md:1752 | "the PR has no `open` Job that is `draft`, `queued` or `running`": a Job kind | internal: Workflow status `open` (docs/commands.md:1873), pull request state `open` (docs/commands.md:2173), command `pr open`, Open Job | overloaded | “its Open Job” (CONTEXT.md capital); say “status `open`” or “state `open`”  |
| merged | docs/commands.md:1753 | "every Job of the PR but its `open` and `update` Jobs `merged` or `cancelled`": Job state | internal: Workflow status `merged` (2168), pull request state `merged` (2173); CONTEXT.md writes Merged | overloaded | “Merged Jobs”, “status `merged`”, “state `merged`”  |
| waiting | docs/commands.md:2129 | "Starts a `waiting` PR whose needs are all `merged`": Workflow status | internal: Session state waiting; upstream: GitHub Actions run `status=waiting` (1830) | overloaded | keep; always “status `waiting`”  |
| cancelled | docs/commands.md:2026 | "`cancelled` is the PR status that stands for a closed PR": says a closed pull request is a cancelled Workflow | upstream: pull request state `closed` (docs/commands.md:1836 "the poll never ends the PR: it stays `open` in cpd"); internal: Job `cancelled` | wrong | “`cancelled` is the Workflow status of a Workflow given up”  |
| status, state | docs/commands.md:2173 | "`state` (`open`, `closed` or `merged`)": the pull request's `state`, inside the same object as the Workflow's `key` and `status` | internal: Workflow `status`; upstream: GitHub `state` | loose | glossary: “Workflow status”, “pull request state”; two objects in the JSON (see 2173 row below)  |
| draft | docs/commands.md:1773 | "`gh pr create` never has `--draft`": a draft pull request | internal: Draft Job (docs/commands.md:1752 "`draft`, `queued` or `running`"); upstream: GitHub draft | overloaded | “draft Job” and “draft pull request”, never bare “draft”  |
| Ready | docs/commands.md:2146 | "The PR's `queued` Jobs are now Ready (§3.2)": a Ready Job | upstream: a pull request "Opened ready" (docs/commands.md:1771); Inbox kind `pr-ready`, "ready for review" (docs/commands.md:2225) | overloaded | “Ready Jobs”; “a pull request that is not a draft”  |
| lock | docs/commands.md:2021 | "Changes a `waiting` PR before the lock (A13)": the Plan lock | internal: file lock (docs/commands.md:1828 "`flock` on `locks/pr-poll.lock`, held for the whole poll"), Locked fields (docs/commands.md:2028), "No lock" (docs/commands.md:1780) | overloaded | “before the Plan lock”; “file lock”; “Locked” only for fields  |
| check | docs/commands.md:1829 | "**The checks read** (pro-d75, contract §7.6 step 3b)": CI checks of a pull request | internal: Mechanical checks, Quick check, "Conflict check" (docs/commands.md:1782) | overloaded | “The CI checks read”  |
| review | docs/commands.md:1781 | "every pending review of the Plan is closed `superseded`": a Plan review | internal: review Job (docs/commands.md:1880 "work, review, open, update"); upstream: GitHub review (docs/commands.md:2173 `reviews`, `review_decision`) | overloaded | “Plan review”, “review Job”, “pull request review”  |
| merge | docs/commands.md:1737 | "its Worker merges the base": git merge of the base into the Lead Branch | internal: a Job's merge into the Lead Branch (docs/commands.md:1780 "merges into the PR go on during the command"); upstream: squash merge (docs/commands.md:2312 "Merges a pull request on GitHub"), `merge_state` | overloaded | “merges the base”, “a Job's merge”, “squash-merge the pull request”  |
| run | docs/commands.md:1830 | "actions/workflows/<file>/runs?status=waiting": a GitHub Actions run | internal: `run/<plan>/logs` directory, "runs the Gate" (docs/commands.md:1739); CONTEXT.md:156 Attempt avoids run | loose | “GitHub Actions run” where it is one  |
| owner, owned | docs/commands.md:1829 | "when a PR a Plan owns (`prs.status` `open`) has an `updatedAt`": a pull request a Workflow points to | internal: the Context's owner (GitHub login, Policy key); `<owner>/<repo>` | overloaded | “a pull request a Workflow points to” (1832 already says “whose PR points to the pull request”)  |
| ticket | docs/commands.md:1951 | "the PR's own Jira ticket, a key matching": a Jira issue key stored on a Workflow | internal: CONTEXT.md:10 levels table has Ticket as the Plan; upstream: Jira | overloaded | “the Workflow's own Jira issue key”  |
| base | docs/commands.md:2173 | "`base` (the pull request's base branch on GitHub, `baseRefName`)": the pull request's base | internal: the Workflow's stored `base` (docs/commands.md:1833 "differs from the PR's `base`"), `base_pr`; git "merges the base" (docs/commands.md:1737) | overloaded | “base branch of the pull request”, “stored base of the Workflow”, “base Workflow”  |
| base PR | docs/commands.md:1833 | "This is what happens when a base PR merges: GitHub deletes its branch": the base's pull request merges | internal: base PR is a Workflow reference (docs/commands.md:1758 "the PR's base PR") | loose | “when a base Workflow's pull request merges”  |
| stacked PR | docs/commands.md:1771 | "GitHub will not land the stacked pull request before the base PR": the Workflow is stacked, its pull request targets the base branch | upstream: GitHub pull request base | loose | “A stacked Workflow: its pull request targets the base Workflow's Lead Branch”  |
| adopt | docs/commands.md:1775 | "adopted PR** (`head_ref` set, §9.4)": a Workflow that adopted an existing pull request; `adopt_*` codes name the pull request | internal: object vs pull request | loose | “A Workflow that adopted a pull request”  |
| post | docs/commands.md:2225 | "Posts a PR's pull request to Slack for review": a Slack announcement | upstream: Slack message; GitHub "the posted comment" (docs/commands.md:2211) of `pr reply` | loose | “announces the pull request in Slack” is a proposal; the PR post entry of CONTEXT.md keeps the word  |
| queue | docs/commands.md:1838 | "The PR's status is the queue (§3.8)": status as work list | internal: Job `queued`; Event "queue item" (CONTEXT.md:167 avoids it); reaction "queue row" (docs/commands.md:2173) | loose | “The Workflow's status says what the engine does next (§3.8)”  |
| hold | docs/commands.md:2087 | "its `merged` Jobs no longer hold it": Jobs keep a Workflow from being cancelled | internal: incident hold (docs/commands.md:1767 "the resolved PR's Plan is not held"); a draft holds the Open Job (CONTEXT.md:136) | overloaded | “its Merged Jobs no longer block it”; “held” only for the incident  |
| step | docs/commands.md:1737 | "(contract §7.6 steps 0, 1, 3, 4)": numbered steps of a contract algorithm | internal: CPD flow steps (1739); the field `step` (docs/commands.md:2141 "`{pr, step, tail?, log?}`"); "Steps:" (docs/commands.md:2314) | overloaded | “contract §7.6 steps” as is; call the field `phase`-free names later (the User's)  |
| `pr_not_ready` | docs/commands.md:2137 | "every need is `merged` (`details.needs` lists those that are not, as keys)": a Workflow whose needs or base are not ready | the same code at 2251 means a pull request that is not postable (`reasons`: draft, changes_requested, ...) | wrong | two codes (the second keeps `pr`-free wording about the pull request), or one with a stated meaning  |
| `pr_not_ready` | docs/commands.md:2251 | "`{pr, number, url, reasons:": the pull request is not postable | the same code at 2137 | wrong | see the row above  |
| `pr` key | docs/commands.md:1794 | `"pr": {"key": "api", "status": "open", "pr": {"number": 812`: outer `pr` is the Workflow, inner `pr` is the pull request | internal vs upstream, in one object | overloaded | `“workflow”: {..., “pull_request”: {“number”: 812`  |
| `pr` key | docs/commands.md:2173 | "`pr`, the PR's `key`, `number`, `url`, `title`, `jira`": Workflow `key`, `title`, `jira` beside pull request `number`, `url` | internal vs upstream, in one object | loose | two objects: `workflow {key, title, jira, base_pr}` and `pull_request {number, url, state, ...}`  |
| `gate.pr` | docs/commands.md:1739 | "or `gate.pr` is `local` (for `pr rebase`)": a policy key; here `pr` is the point at which the Gate runs | internal: CONTEXT.md:69 writes `pr.gate`; contract.md:3963 gives `pr` the values `local` or `none` while 1739 says `ci` is the default | loose | one spelling in both documents; unverified which is current  |
| Policy domain `pr` | docs/commands.md:1756 | "the PR's policy sets `pr.jira_required` (B4, pro-d127)": a Workflow's Jira rule inside the domain `pr` | `pr.post.*` (docs/commands.md:2236 "`policy.pr.post.enabled` is false") and `pr.feedback.checks` (docs/commands.md:1829) are about the pull request; CONTEXT.md:268 lists the domain `pr` | overloaded | decision for the User: keep domain `pr` for the pull request and move `jira_required`, or split the domain  |
| `PR react` | docs/commands.md:2260 | "(PR `react`)": the PR key `react` of Plan `pr-post`, a record reference with no Plan named | internal: reads as an operation | loose | “(Workflow `react` of Plan `pr-post`)”, as docs/commands.md:4721 spells it with the Plan  |

Terms that are fine in this part (used in one sense, as CONTEXT.md defines them; checked by reading, not exhaustive): Plan, Plan phase (`grounding`, `plan`, `dispatch`, `deploy`, `done` used only as Plan phases, with `phase` as the JSON key and `plan-phase` as the Event), Context (entity), Lead, Worker, Job, Job kind, Open Job, Update Job, Spec, Review Job, Attempt, Ledger (1891), Event, Inbox, Inbox item, Cockpit, Engine, PR poll, Feedback item, PR post, User, Manager, Escalation (1891), Gate, Lead Branch, Lead worktree, Tier, Policy, Policy key, Repo policy file, Session; the words epic, finding, brief, requirement, assumption, outline, decision, note, board, task, park and archive are not used in a clashing sense here (`task`, `epic`, `brief`, `outline`, `finding`, `board` do not occur). The Escalation ids `nl-pr-a1-q1` and `nl-pr-a1-q2` (1891) embed an old Plan name and are history, kept.

## The pr commands

Legend for the JSON and code column: (o) object, (p) pull request, (m) mixed, (x) other (git, Gate, policy, Slack, lock, Attempt). "Acts on" quotes the sentence that says it. The last column is one of: **stays under `pr`** (it acts on the pull request), **moves under `workflow`** (it acts on the Workflow), **decision** (both). The four rows for `host approve`, `host repair` and the two `tui action` commands name no PR, so none of the three applies; they sit in section 9 by position only and are shown so the table covers the whole part.

| Command | What it acts on (a Workflow, its pull request, a GitHub pull request by owner/repo#number, or both) | Flags and what each names | JSON fields and error codes with senses | Where it should live after the rename |
|---|---|---|---|---|
| `pr open` (9.1, 1735) | both: "it runs the Gate, then opens the pull request" (1739); the status check is on the Workflow ("the PR is `dispatch`", 1751) | `--pr <key>` names a Workflow (1747); `--plan <id>` | JSON `phase` (x, Plan phase), `pr.key`, `pr.status` (o), inner `pr.number`, `url`, `opened_at` (p), `adopted`, `sha`, `pushed`, `remote`; codes `illegal_transition`, `open_job_live`, `jobs_not_done`, `pr_title_required`, `description_required`, `jira_required` (o), `pr_closed` (p), `base_not_found`, `base_conflict`, `owner_unset`, `owner_unconfirmed`, `policy_invalid`, `gate_error`, `gate_failed`, `cas_lost`, `incident_hold`, `push_rejected`, `git_failed`, `gh_failed` (x); Events `pr-opened` (m), `pr-merged` (m), `plan-phase`, `plan-done` | decision: reading 1 `workflow open` (acts on the Workflow: status `dispatch` to `open`, title, description, Jira, Gate, push); reading 2 `pr open` (it opens the pull request) |
| `pr poll` (9.2, 1805) | GitHub pull requests: "one poll of **all of the User's open pull requests**" (1811); `--plan` adds that Plan's Workflows to the output | `--plan <id>` adds the Plan's Workflows (1815); no `--pr` (1733) | JSON `polled`, `changed` (p), `base_changed`, `feedback`, `merged`, `done` (o, `{plan, pr}` pairs), `update_jobs` (o), `rate_limit` (x), `prs` (o, each with a `pull_request` row, p); codes `lock_timeout`, `plan_not_found`, `gh_failed` (x); Events `pr-feedback` (p), `pr-merged` (m), `pr-base-changed` (o), `job-added`, `plan-done` | stays under `pr` |
| `pr rebase` (9.3, 1857) | both: "Rebases the PR's Lead Branch onto its base and pushes (§7.6)" (1863); the Lead Branch is the Workflow's, the push reaches the pull request | `--pr <key>` names a Workflow (1867); `--continue`, `--abort`; `--plan <id>` | JSON `pr` (o, a key string), `rebased`, `old`, `new`, `pushed`; codes `illegal_transition`, `stacked_rebase`, `jobs_not_done` (o), `rebase_conflict`, `rebase_in_progress`, `lead_worktree_dirty`, `cas_lost`, `attempt_running`, `owner_unset`, `policy_invalid`, `gate_error`, `gate_failed`, `incident_hold`, `git_failed` (x) | decision: reading 1 `workflow rebase` (Lead Branch and Lead worktree are the Workflow's); reading 2 `pr rebase` (it publishes the pull request's branch) |
| `pr rebase --continue` (1909) | both, as `pr rebase`: "Finishes the stopped rebase, then the Gate and the lease push" (1911) | `--continue` (a mode of `pr rebase`); `--pr <key>`: a Workflow | JSON as `pr rebase`; codes `usage`, `illegal_transition` (o), `no_rebase_in_progress`, `rebase_conflict`, `cas_lost`, `git_failed`, `gate_error`, `gate_failed` (x) | decision: as `pr rebase`; it moves with it |
| `pr rebase --abort` (1929) | the Workflow's Lead worktree: "the Lead Branch and the Lead worktree as before the stop" (1931) | `--abort`; `--pr <key>`: a Workflow | JSON `pr` (o), `aborted`, `tip`; code `no_rebase_in_progress` (x) | decision: as `pr rebase`; it asks no pull request and no status, so it leans to `workflow` |
| `pr add` (9.4, 1933) | a Workflow: "Adds a PR to the Plan, `waiting` (R34, A13, §3.8)" (1939); `--adopt` names an existing pull request | positional `<key>` names a Workflow (1943); `--base <plan>/<key>` names the base Workflow; `--needs`, `--no-needs` name Workflows; `--adopt <number>`, `--confirm-author <login>` name a pull request and its author; `--repo`, `--title`, `--jira`, `--shape` | JSON `pr.key`, `repo`, `name`, `base`, `base_pr`, `title`, `jira`, `needs`, `status`, `lead_branch`, `head_ref`, `appended`, `jobs` (o); codes `pr_exists`, `base_pr_unknown`, `pr_other_plan`, `base_pr_unavailable`, `needs_unavailable`, `base_pr_other_repo`, `needs_required`, `needs_unknown`, `dependency_cycle` (o), `adopt_not_open`, `adopt_fork`, `adopt_foreign` (p), `adopt_cpd_branch`, `adopt_taken` (m), `archived`, `not_a_git_repo`, `owner_*`, `policy_invalid`, `repo_unkeyed`, `base_not_found`, `branch_exists`, `tests_unconfigured`, `job_key_exists`, `gh_failed` (x); Events `pr-added`, `job-added`, `plan-phase` | moves under `workflow` |
| `pr edit` (9.5, 2015) | a Workflow: "Changes a `waiting` PR before the lock (A13)" (2021); `--title` also retitles its open pull request | positional `<key>` names a Workflow; `--base <plan>/<key>`, `--needs`, `--no-needs` name Workflows; `--repo`, `--title`, `--jira` | JSON `pr.key`, `pr.status`, `fields`, `already` (o); codes `pr_adopted`, `pr_ended`, `pr_locked`, `pr_in_use`, `illegal_transition`, `needs_unknown`, `needs_unavailable`, `dependency_cycle` (o), `nothing_to_edit`, `repo_unkeyed`, `owner_mismatch`, `base_not_found`, `branch_exists`, `gh_failed` (x); Event `pr-edited` | moves under `workflow` |
| `pr describe` (9.5a, 2057) | both: "Stores the PR's description, verbatim, which becomes the pull request body" (2063) | `--pr <key>` names a Workflow (2059); `--file <path>`; `--plan <id>` | JSON `pr.key`, `pr.status` (o), `already`; codes `pr_ended`, `pr_adopted`, `description_invalid` (o), `file_unreadable`, `gh_failed` (x); Event `pr-described` | decision: reading 1 `workflow describe` (the description is a Workflow column, set before the pull request exists); reading 2 `pr describe` (it is the pull request body) |
| `pr cancel` (9.6, 2081) | a Workflow: "Before the lock it removes a `waiting` PR from the Plan" (2089) | positional `<key>` names a Workflow; `--superseded-by <plan>/<key>` names the superseding Workflow; `--plan <id>` | JSON `pr.key`, `pr.status`, `phase` (x), `released` `{number, url, lead_worktree, lead_branch}` (m: pull request `number`, `url` beside the Workflow's worktree and branch); codes `illegal_transition`, `pr_not_found`, `pr_in_use` (o), `pr_not_closed` (m), `usage`, `cas_lost` (x); Events `pr-cancelled`, `job-cancelled`, `plan-phase`, `plan-done` | moves under `workflow` |
| `pr start` (9.7, 2123) | a Workflow: "Starts a `waiting` PR whose needs are all `merged`" (2129) | positional `<key>` names a Workflow; `--plan <id>` | JSON `pr.key`, `status`, `lead_branch`, `lead_worktree`, `base_sha`, `reused`, `pushed` (o); codes `illegal_phase` (x, Plan phase), `illegal_transition`, `pr_not_ready` (o), `worktree_exists`, `branch_exists`, `setup_failed` (x), `pr_start_failed` (o, error code and `step` field); Events `pr-started`, `pr-base-changed` | moves under `workflow` |
| `pr view` (9.8, 2154) | a GitHub pull request, selected through a Workflow: "the checks, reviews and comments of one PR's pull request" (2160) | `--pr <key>` names a Workflow (2164); `--plan <id>` | JSON `pr.key`, `title`, `jira`, `base_pr` (o) with `pr.number`, `url`, `base`, `state`, `review_decision`, `checks` (p) in one object (m), `checks`, `reviews`, `comments`, `inline_comments`, `feedback`, `post` (p); codes `illegal_transition` (o), `gh_failed` (x) | stays under `pr` (reads the pull request; its selector flag is renamed) |
| `pr reply` (9.8a, 2187) | a Feedback item of a pull request: "Answers one feedback item" (2193), selected through a Workflow | `--pr <key>` names a Workflow (2197); `--to <item-id>` names a Feedback item of the pull request (2198); `--body`, `--body-file` | JSON `replied.pr` (o, a key), `to`, `url` (p); codes `pr_not_open` (o), `feedback_not_found` (p), `usage`, `file_unreadable`, `gh_failed` (x) | stays under `pr` |
| `pr post` (9.8b, 2219) | both: "Posts a PR's pull request to Slack for review" (2225); the post is stored per Workflow (`pr_posts`) | `--pr <key>` names a Workflow (2229); `--plan <id>` | JSON `pr` (o, a key), `number`, `url` (p), `post`, `message` (x, Slack); codes `pr_not_open` (o), `already_posted` (m), `not_authored`, `pr_not_ready` (p), `post_disabled`, `post_unconfigured`, `policy_invalid`, `owner_unset`, `owner_unconfirmed`, `slack_*`, `spawn_failed`, `store_failed` (x); policy keys `pr.post.*` (m); table `pr_posts` (m) | stays under `pr` (the pull request is announced); the stored post is keyed by Workflow, so the User may prefer decision |
| `pr push` (9.9, 2270) | both: "Publishes an `open` PR's Lead Branch with a plain push" (2276) | `--pr <key>` names a Workflow (2280); `--plan <id>` | JSON `pr` (o, a key), `pushed`, `sha`, `remote`; codes `illegal_transition` (o), `gate_error`, `gate_failed`, `owner_unset`, `policy_invalid`, `push_rejected`, `incident_hold`, `git_failed` (x) | decision: reading 1 `workflow push` (the Lead Branch is the Workflow's); reading 2 `pr push` (it reaches the pull request) |
| `pr merge` (9.10, 2306) | a GitHub pull request by `<owner>/<repo>#<n>`: "It takes a pull request, not a Plan's PR key" (2312); its effects also move a Workflow when a `prs` row points to it (2320) | `<owner>/<repo>#<n>` names a pull request; no `--pr` | JSON `repo`, `number`, `method`, `branch_deleted`, `stored` (p); codes `usage`, `merge_not_clean` (p), `gh_failed` (x); Events `pr-merged` (m), `plan-done` | stays under `pr` |
| `host approve` (9.11, 2338) | no PR: "creates a private approval file for the Session" (2347) | none | JSON `ok` only | not a `pr` command; names no PR, so nothing moves (stays where it is, under `host`, and its place in section 9 is a placement question) |
| `host repair` (9.11, 2338) | no PR: "repairs the skipped cpd install by resetting the checkout's main branch to origin/main" (2349) | none | JSON `saved`, `head`; code `git_failed` (x) | not a `pr` command; names no PR, so nothing moves (stays under `host`) |
| `tui action host` (9.12, 2351) | no PR: "what a click on the red version pill runs when `@cpd_host_alert` is set" (2353) | none | prints nothing | not a `pr` command; names no PR, so nothing moves (stays under `tui`) |
| `tui action incident` (9.13, 2355) | no PR: "what a click on the incident pill runs (contract §6.1)" (2357) | none | prints nothing | not a `pr` command; names no PR, so nothing moves (stays under `tui`) |
| flag `--pr <key>` (27 mentions in the part) | a Workflow, by key: "required when the Plan has two or more PRs that are not `cancelled`" (1747,1867,2229,2280) | on `open`, `rebase`, `push`, `view`, `reply`, `post`, `describe` | selected by §1.3's rule (omitted: the Plan's one non-cancelled Workflow) | decision: the flag names a Workflow, so it is `--workflow <key>` wherever it stays; commands that stay under `pr` keep a Workflow selector |
| positional `<key>` | a Workflow: "PR key (§2.1), chosen by the Lead" (1943) | on `add`, `edit`, `cancel`, `start` | none | moves under `workflow` (it follows `add`, `edit`, `cancel`, `start`) |
| flag `--superseded-by <ref>` (2087) | a Workflow: "its work was carried by another PR" (2087) | `<ref>` is `<plan>/<key>` or a bare key | Event data `superseded_by` (o) | moves under `workflow` (flag of `pr cancel`) |
| flag `--base <plan>/<key>` (1947) | a base Workflow: "**stack on a PR**" (1947) | on `pr add` and `pr edit`; the other `--base <branch>` names a git branch (x) | `base`, `base_pr` (o) | moves under `workflow` (flag of `add`, `edit`) |
| flags `--needs`, `--no-needs` (1948) | Workflows: "PR references (contract §2.2): keys of other PRs of the Plan" (1948) | on `pr add` and `pr edit` | `needs`, table `pr_needs` (o) | moves under `workflow` |
| flag `--to <id>` (2198) | a Feedback item: "the `id` of an item in the PR's stored `feedback` list" (2198) | on `pr reply`; `<id>` printed by `pr view` | `feedback_not_found` (p) | stays under `pr` |
| argument `<owner>/<repo>#<n>` (2312) | a GitHub pull request: "`<owner>/<repo>#<n>` is the form the Deploy column shows" (2312) | on `pr merge` | none | stays under `pr` |
| flags `--adopt <number>`, `--confirm-author <login>` (1952) | a pull request: "adopt the existing pull request `<number>` of `--repo`'s `origin`" (1952) | on `pr add` only | `adopt_*` codes (p, m), `head_ref`, `author` | moves under `workflow` (flags of `add`; the value names a pull request) |
| flag `--pr <key>` of `job add` (1783) and `policy show --pr <key>` (2028) | a Workflow: "the Lead appends fix Jobs to it (`job add --pr <key>`)" (1783) | referenced here, defined in other parts | none | decision: not `pr` commands; the flag is renamed, the command stays (audited by the part that defines it) |

## PR occurrences

Groups are by subsection, with the count of `PR`/`PRs` tokens. Every `mixed`, `pull request` and `other` token, and the one `step` use, is listed after the groups.

**Groups of `object` tokens** (rule: every such token becomes Workflow; singular or plural):

| Where (section or path:line range) | Count | Sense | Quote | After the rename  |
|---|---|---|---|---|
| docs/commands.md:1731-1734 (9 intro) | 6 | object | "Every command here works on **one PR**" | Workflow  |
| docs/commands.md:1735-1804 (9.1 `pr open`) | 57 | object | "the PR has no `open` Job that is `draft`, `queued` or `running`" | Workflow  |
| docs/commands.md:1805-1856 (9.2 `pr poll`) | 42 | object | "adds that Plan's PRs, each with its `pull_request` row" | Workflow  |
| docs/commands.md:1857-1908 (9.3 `pr rebase`) | 25 | object | "Rebases the PR's Lead Branch onto its base and pushes" | Workflow  |
| docs/commands.md:1909-1928 (`pr rebase --continue`) | 2 | object | "the PR is `open`, before the Gate and again after it" | Workflow  |
| docs/commands.md:1929-1932 (`pr rebase --abort`) | 1 | object | "It asks no PR status, runs no Gate and no `fetch`" | Workflow  |
| docs/commands.md:1933-2014 (9.4 `pr add`) | 87 | object | "Adds a PR to the Plan, `waiting`" | Workflow  |
| docs/commands.md:2015-2056 (9.5 `pr edit`) | 47 | object | "Changes a `waiting` PR before the lock (A13)" | Workflow  |
| docs/commands.md:2057-2080 (9.5a `pr describe`) | 9 | object | "Stores the PR's description, verbatim" | Workflow  |
| docs/commands.md:2081-2122 (9.6 `pr cancel`) | 48 | object | "it removes a `waiting` PR from the Plan" | Workflow  |
| docs/commands.md:2123-2153 (9.7 `pr start`) | 25 | object | "Starts a `waiting` PR whose needs are all `merged`" | Workflow  |
| docs/commands.md:2154-2186 (9.8 `pr view`) | 9 | object | "the PR to view, selected by §1.3's rule" | Workflow  |
| docs/commands.md:2187-2218 (9.8a `pr reply`) | 4 | object | "the PR, selected by §1.3's rule" | Workflow  |
| docs/commands.md:2219-2269 (9.8b `pr post`) | 8 | object | "the PR to post, selected by §1.3's rule" | Workflow  |
| docs/commands.md:2270-2305 (9.9 `pr push`) | 12 | object | "Publishes an `open` PR's Lead Branch with a plain push" | Workflow  |
| docs/commands.md:2306-2337 (9.10 `pr merge`) | 6 | object | "not a Plan's PR key" | Workflow  |
| docs/commands.md:2338-2358 (9.11-9.13) | 0 | none | no `PR` token | unchanged  |

**`mixed` tokens, listed individually** (each says why):

| Where (section or path:line range) | Count | Sense | Quote | After the rename  |
|---|---|---|---|---|
| docs/commands.md:1737 | 1 | mixed | "A PR is opened by its `open` Job": the Workflow is not opened; its pull request is | “A Workflow's pull request is opened by its `open` Job”  |
| docs/commands.md:1737 | 1 | mixed | "so this is how the PR is opened later": same, the pull request is opened | “so this is how the pull request is opened later”  |
| docs/commands.md:1771 | 1 | mixed | "GitHub will not land the stacked pull request before the base PR": the base's pull request lands | “before the base Workflow's pull request”  |
| docs/commands.md:1771 | 1 | mixed | "Opened ready, never a draft, like every PR.": ready and draft are pull request states | “like every pull request”  |
| docs/commands.md:1829 | 1 | mixed | "(a) the PR is owned and neither search listed it": owned by a Workflow, listed by a GitHub search | “the pull request is owned by a Workflow and neither search listed it”  |
| docs/commands.md:1829 | 1 | mixed | "when a PR a Plan owns (`prs.status` `open`) has an `updatedAt`": a Workflow status, a pull request field | “when a pull request a Workflow points to (status `open`) has an `updatedAt`”  |
| docs/commands.md:1829 | 1 | mixed | "`details` name the PR and the number": the Workflow key and the pull request number | “name the Workflow and the pull request number”  |
| docs/commands.md:1829 | 1 | mixed | "(a) the PR is owned, (b) the PR's `pr.feedback.checks` is `any`": owned pull request | “the pull request is owned”  |
| docs/commands.md:1829 | 1 | mixed | "details name the PR and the number": as above (the checks read) | “name the Workflow and the pull request number”  |
| docs/commands.md:1832 | 1 | mixed | "when a reopened Plan's last PR merges": the pull request merges, the Workflow becomes `merged` | “when a reopened Plan's last Workflow becomes merged”  |
| docs/commands.md:1833 | 1 | mixed | "This is what happens when a base PR merges: GitHub deletes its branch": the pull request merges on GitHub | “when a base Workflow's pull request merges”  |
| docs/commands.md:1833 | 1 | mixed | "so a ready PR stays ready (hs-d326)": ready is a pull request state | “so a ready pull request stays ready”  |
| docs/commands.md:1833 | 1 | mixed | "A base PR closed unmerged is not repointed by GitHub": closed is a pull request state | “A base Workflow's pull request closed unmerged is not repointed”  |
| docs/commands.md:1939 | 1 | mixed | "a merged PR can hold a bug or a regression": the merged code is the pull request's | “a merged Workflow can hold a bug” (or “merged pull request”; the User's)  |
| docs/commands.md:2026 | 1 | mixed | "`cancelled` is the PR status that stands for a closed PR": status of the Workflow, closed of the pull request | “`cancelled` is the Workflow status of a Workflow given up”  |
| docs/commands.md:2173 | 1 | mixed | "`pr`, the PR's `key`, `number`, `url`, `title`, `jira`": `key` is a Workflow field, `number` and `url` are the pull request's | split into `workflow` and `pull_request`  |
| docs/commands.md:2173 | 1 | mixed | "null for an unposted PR": the post announces the pull request but is kept per Workflow | “null for a Workflow whose pull request is unposted” (guess)  |
| docs/commands.md:2225 | 1 | mixed | "A PR may be posted only once": the pull request is posted, the limit is per Workflow | “A pull request may be posted only once” (the User checks)  |
| docs/commands.md:2260 | 1 | mixed | "a PR the poll never read for comments is `comments_unread`": the poll reads the pull request | “a pull request the poll never read for comments”  |
| docs/commands.md:2312 | 1 | mixed | "after a peer approves a posted PR": approval is of the pull request; the post is of a Workflow | “after a peer approves a posted pull request”  |
| docs/commands.md:2312 | 1 | mixed | "with reason `approved` of a posted PR": same | “of a posted pull request”  |

**`pull request`, `other`, `step`, `phase`** (listed individually):

| Where (section or path:line range) | Count | Sense | Quote | After the rename  |
|---|---|---|---|---|
| docs/commands.md:1811 | 1 | pull request | "The PR poller (hs-d96, D8; contract §7.6)": PR poll, polls pull requests | unchanged  |
| docs/commands.md:1829 | 1 | pull request | "stores the result in the PR's `checks` field": the `checks` of the pull request | unchanged  |
| docs/commands.md:1830 | 1 | pull request | "the PR poll's own writes standing": the PR poll | unchanged  |
| docs/commands.md:1831 | 1 | pull request | "set by searches that list the PR (hs-d565)": a search lists pull requests | unchanged  |
| docs/commands.md:2320 | 1 | pull request | "GitHub's search listed the PR after its merge": the search lists pull requests | unchanged  |
| docs/commands.md:2260 | 1 | other | "(PR `react`)": the PR key `react` of Plan `pr-post` (history reference, docs/commands.md:4721) | “(Workflow `react` of Plan `pr-post`)”  |
| docs/commands.md:1739 | 1 | step | "`pr open` is the **Mechanical checks** step of the Deploy stage": covers the Mechanical checks and PR steps | name both steps, new name of the PR step is the User's  |
| docs/commands.md:1731-2358 | 0 | phase | no use of the old PR phase (see Coverage) | not applicable  |

**`pull request` words** (the 94 words `pull request(s)`, all `pull request`; unchanged):

| Where (section or path:line range) | Count | Sense | Quote | After the rename  |
|---|---|---|---|---|
| docs/commands.md:1731-1734 | 2 | pull request | "and their pull requests, through `gh` (§7.6)" | unchanged  |
| docs/commands.md:1735-1804 | 20 | pull request | "the pull request is opened only after the conflict check above" | unchanged  |
| docs/commands.md:1805-1856 | 24 | pull request | "one poll of **all of the User's open pull requests**" | unchanged  |
| docs/commands.md:1933-2014 | 20 | pull request | "it adopts an existing pull request whose head branch does not equal" | unchanged  |
| docs/commands.md:2015-2056 | 4 | pull request | "When the PR has an **open** pull request, the command also retitles it" | unchanged  |
| docs/commands.md:2057-2080 | 6 | pull request | "When the PR has an **open** pull request, the command also updates it" | unchanged  |
| docs/commands.md:2081-2122 | 2 | pull request | "an **`open`** PR whose pull request was closed on GitHub without a merge" | unchanged  |
| docs/commands.md:2154-2186 | 4 | pull request | "of one PR's pull request, which the poller does not store" | unchanged  |
| docs/commands.md:2187-2218 | 1 | pull request | "<the pull request's node id>" | unchanged  |
| docs/commands.md:2219-2269 | 4 | pull request | "Posts a PR's pull request to Slack for review" | unchanged  |
| docs/commands.md:2270-2305 | 1 | pull request | "only need to reach the pull request" | unchanged  |
| docs/commands.md:2306-2337 | 6 | pull request | "Merges a pull request on GitHub" | unchanged  |

**Identifiers** (the lowercase and snake-case forms; counts from the regexes in `## Counts`):

| Where (section or path:line range) | Count | Sense | Quote | After the rename  |
|---|---|---|---|---|
| docs/commands.md:1733 (count over 1731-2358), flag `--pr` | 27 | object | "select it by `--pr <key>`" (the flag names a Workflow) | `--workflow <key>`  |
| docs/commands.md:2003 (count over 1731-2358), table `prs`, `prs.*` columns, keys `prs` and `details.prs` | 19 | object | "`prs` (`status = waiting`, `repo`, `name`, `base`" | table `workflows` (the User names it)  |
| docs/commands.md:2003 (count over 1731-2358), `pr_needs`, `base_pr*`, `from_pr`, `to_pr` | 18 | object | "one `pr_needs` row per need" | `workflow_needs`, `base_workflow*`  |
| docs/commands.md:2146 (count over 1731-2358), Events `pr-started`, `pr-base-changed`, `pr-added`, `pr-edited`, `pr-described`, `pr-cancelled` | 11 | object | "**Event** `pr-started`" | `workflow-started` and the like  |
| docs/commands.md:1787 (count over 1731-2358), Events `pr-opened`, `pr-merged` | 7 | mixed | "**Event** `pr-opened` `{pr, number, url}`": data has the Workflow key and the pull request number and url | decision: `workflow-opened` or `pull-request-opened`, as the User reads it  |
| docs/commands.md:1832 (count over 1731-2358), Events `pr-feedback`, `pr-ready` | 12 | pull request | "`pr-feedback` (data of contract §5.2)" | unchanged  |
| docs/commands.md:1787 (count over 1731-2358), table `pull_requests`, column `pull_request` | 15 | pull request | "the `pull_requests` row of the pull request" | unchanged  |
| docs/commands.md:1781 (count over 1731-2358), `pr_number`, `pr_url`, `pr_opened_at` | 8 | mixed | "the PR's `pr_number`, `pr_url`, `pull_request`" | columns of the Workflow that hold the pull request's data: decision  |
| docs/commands.md:2234 (count over 1731-2358), table `pr_posts` | 2 | mixed | "a `pr_posts` row exists for the PR" | decision, with `pr post`  |
| docs/commands.md:2236 (count over 1731-2358), policy keys `pr.post.*` | 10 | mixed | "`policy.pr.post.enabled` is false" | decision, with the domain `pr`  |
| docs/commands.md:1756 (count over 1731-2358), policy key `pr.jira_required` | 3 | object | "the PR's policy sets `pr.jira_required` (B4, pro-d127)" | decision, with the domain `pr`  |
| docs/commands.md:1829 (count over 1731-2358), policy key `pr.feedback.checks` | 4 | pull request | "(b) the PR's `pr.feedback.checks` is `any`" | unchanged  |
| docs/commands.md:1739 (count over 1731-2358), policy keys `gate.pr`, `gate.merge` | 17 | mixed | "`gate.merge` is `local` (for `pr open` and `pr push`)" | decision, with the domain name  |
| docs/commands.md:2021 (count over 1731-2358), error codes `pr_in_use`, `pr_locked`, `pr_ended`, `pr_adopted`, `pr_exists`, `pr_other_plan`, `pr_not_found`, `pr_not_open`, `pr_title_required`, `pr_start_failed` | 34 | object | "refused `pr_in_use` (`details.prs`)" | `workflow_*`  |
| docs/commands.md:1757 (count over 1731-2358), error code `pr_closed` | 2 | pull request | "the newest pull request of the Lead Branch is not closed without a merge" | unchanged  |
| docs/commands.md:2137 (count over 1731-2358), error codes `pr_not_ready`, `pr_not_closed` | 5 | mixed | "every need is `merged` (`details.needs` lists those that are not, as keys)" | split `pr_not_ready` into two codes (see Terms)  |
| docs/commands.md:1828 (count over 1731-2358), `pr-poll.lock`, `pr-poll-failing.json`, `watch.pr_poll_seconds`, `watch.pr_poll_transient_minutes`, `pr_poll_seconds` | 7 | pull request | "`flock` on `locks/pr-poll.lock`" | unchanged  |
| docs/commands.md:1763 (count over 1731-2358), log files `pr-<key>.pre-start.log`, `pr-<key>.pre-merge.log` | 10 | object | "`run/<plan>/logs/pr-<key>.pre-start.log`" | `workflow-<key>.pre-start.log`  |

## Fix list

**Rules** (mechanical, no judgement; the counts are per section as in `## PR occurrences`):

| path:line | what to change | why |
|---|---|---|
| docs/commands.md:1731-2358 | RULE 1: every `object` token `PR`/`PRs` becomes Workflow/Workflows: 388 tokens (1731-1734: 6; 9.1: 57; 9.2: 42; 9.3: 25+2+1; 9.4: 87; 9.5: 47; 9.5a: 9; 9.6: 48; 9.7: 25; 9.8: 9; 9.8a: 4; 9.8b: 8; 9.9: 12; 9.10: 6) | the Spec's rename; "PR" keeps only the pull request |
| docs/commands.md:1731-2358 | RULE 2: identifiers follow the object: flag `--pr` (27), table `prs` and its columns and keys (19), `pr_needs`, `base_pr*`, `from_pr`, `to_pr` (18), six Event kinds (11), 10 object error codes (34), log files `pr-<key>.*.log` (10), policy key `pr.jira_required` (3) | the rename covers identifiers too (pro-d136, pro-d138) |
| docs/commands.md:1731-2358 | RULE 3: keep as is: 5 `pull request` tokens `PR`, the 94 words pull request(s), `pull_requests`, `pull_request`, `pr-feedback`, `pr-ready`, `pr_closed`, `pr-poll.lock`, `pr.feedback.checks`, `watch.pr_poll_*` | they mean the GitHub pull request |

**Judgement rows:**

| path:line | what to change | why |
|---|---|---|
| docs/commands.md:1737 | reword "A PR is opened by its `open` Job" and "the PR is opened later" to say the pull request is opened | mixed: the object is not opened, its pull request is |
| docs/commands.md:1739 | name the two steps `pr open` repairs (Mechanical checks, and the step that opens the pull request) | step: the sentence calls the whole of it the Mechanical checks step |
| docs/commands.md:1739 | settle the default of `gate.pr`: this line says `ci`, contract.md:3963 gives `pr` the values `local` or `none`; CONTEXT.md:69 writes `pr.gate` | wrong or stale in one of three places; unverified which is current |
| docs/commands.md:1771 | reword "like every PR" and "before the base PR" | mixed: ready, draft and landing are pull request facts |
| docs/commands.md:1781 | delete "The next `pr poll` cleans up the merged PR's worktrees." | stale: docs/commands.md:1838 says "the poll records the status and cleans nothing" and `cpd janitor` removes them |
| docs/commands.md:1794 | split the JSON key `pr` into `workflow` and an inner `pull_request` (also 1800 and the `pr view` object 2173) | mixed: one `pr` key names two objects |
| docs/commands.md:1829 | reword "a PR a Plan owns", "the PR is owned" (twice), "name the PR and the number" (twice) | mixed (five tokens): Workflow key and pull request number in one phrase; "owns" is loose |
| docs/commands.md:1830 | write “GitHub Actions workflow” for “each workflow” and “the failed workflow” | collision with the new Workflow noun |
| docs/commands.md:1831 | make the cockpit column name agree with CONTEXT.md:186 ("Dispatch column") | loose: two names for one column |
| docs/commands.md:1832 | reword "a reopened Plan's last PR merges" | mixed: the pull request merges, the Workflow becomes merged |
| docs/commands.md:1833 | reword "when a base PR merges", "a ready PR stays ready", "A base PR closed unmerged" | mixed (three tokens): pull request states |
| docs/commands.md:1939 | reword "a merged PR can hold a bug or a regression" | mixed: the merged code is in the pull request |
| docs/commands.md:1945,1950 | delete one of the two `--title <text>` rows (same text; 1950 adds the `--adopt` sentence, keep that one) | stale merge residue: duplicate row |
| docs/commands.md:1946,1954 | delete the duplicate `--base <branch>` row; 1954 also says it is exclusive with `--adopt` (keep that one) | stale merge residue: duplicate row |
| docs/commands.md:1947,1955 | delete the duplicate `--base <plan>/<key>` row (keep 1955, it has the `--adopt` rule) | stale merge residue: duplicate row |
| docs/commands.md:1948,1956 | keep one `--needs <r1,r2>` row: 1948 says the default in `dispatch` is required (`needs_required`), 1956 says "in other phases none" | wrong: the two rows contradict each other; 1977 (`needs_required`) supports 1948 |
| docs/commands.md:1949,1957 | keep one `--no-needs` row: 1949 says it "changes the default", 1957 says "also in phase `deploy` or `done`" | wrong: the two rows contradict each other |
| docs/commands.md:1976,1983 | delete one of the two `pr_exists` precondition rows (1983 compares `head_ref` too) | stale merge residue: duplicate row |
| docs/commands.md:2021,2022,2024 | keep one of the three versions of the paragraph "Changes a `waiting` PR before the lock"; 2022 is the one with `pr_adopted` | stale merge residue: three versions of one paragraph, two of which lack the adopted rule |
| docs/commands.md:2026 | reword "`cancelled` is the PR status that stands for a closed PR" | wrong: a closed pull request leaves the Workflow `open` (1836) |
| docs/commands.md:2105,2107,2138,2145,2146 | “workflow worktree” becomes “Lead worktree” | stale cpd 1 word; collides with the new Workflow |
| docs/commands.md:1946,1954 | write “cpd 1's retired `workflow init`” | loose: reads as a live noun after the rename |
| docs/commands.md:2137,2251 | split `pr_not_ready` into two codes, or state the two meanings | wrong: one code means unmet needs (Workflow) and an unpostable pull request |
| docs/commands.md:2173 | split `pr` into `workflow` and `pull_request`; fix "null for an unposted PR" | mixed (two tokens): one object mixes both |
| docs/commands.md:2225 | reword "A PR may be posted only once" | mixed: the pull request is posted; the one-post limit is per Workflow |
| docs/commands.md:2260 | write “(Workflow `react` of Plan `pr-post`)” for “(PR `react`)”; reword “a PR the poll never read for comments” | other (history key with no Plan) and mixed |
| docs/commands.md:2312 | reword "a posted PR" (twice); fix the self-reference "(pro-d64, §9.10)" inside §9.10 (probably §9.8b, unverified) | mixed (two tokens); loose: a section cites itself |
| docs/commands.md:2320,2334,2336 | merge the two "Effects" and "Rows" paragraphs of `pr merge`; define or delete "(A), (B), (C)" | stale residue: the same rules stated twice, labels with no definition |
| docs/commands.md:2338,2351,2355 | decide where 9.11, 9.12, 9.13 live: they name no PR and sit under `## 9. pr` by position only | placement: `host` and `tui action` are not `pr` commands |
| docs/commands.md:2357 | delete the second full stop in "as every `cpd tui action` does.." | typo |
| docs/commands.md:1756,2236,1829 | decide the domain name: `pr.jira_required` (Workflow), `pr.post.*` and `pr.feedback.checks` (pull request) share the domain `pr` | overloaded: one domain, two senses |

## Counts

| What | Number |
|---|---|
| files audited (the part is in one file) | 1 (`docs/commands.md`, lines 1731-2358) |
| files read as reference | 1 in full (`CONTEXT.md`, 323 lines); `docs/contract.md` only by single-line reads |
| lines read | 628 of the part, plus 323 of `CONTEXT.md` |
| rows in `## Terms` | 39 (and one summary paragraph for the terms that are fine) |
| rows in `## The pr commands` | 28 (19 commands and modes, 9 flag and argument rows) |
| rows in `## PR occurrences` | 76 (17 object groups, 21 mixed, 8 for pull request tokens, other, step and phase, 12 pull request word groups, 18 identifier groups) |
| rows in `## Fix list` | 34 (3 rules, 31 judgement rows) |
| `PR` or `PRs` tokens | 415: object 388, mixed 21, pull request 5, other 1, phase 0, step 0 |
| "pull request(s)" words | 94, all `pull request` |
| `step` uses (no `PR` token) | 1 (line 1739) |
| `phase` uses of the old PR phase | 0 |
| identifiers, by sense | object 122 (`--pr` 27, `prs` 19, `pr_needs` and `base_pr*` 18, six Events 11, ten codes 34, log files 10, `pr.jira_required` 3); pull request 40 (Events 12, `pull_request(s)` 15, `pr.feedback.checks` 4, `pr_closed` 2, lock, state and config names 7); mixed 49 (`pr-opened` and `pr-merged` 7, `pr_number` and others 8, `pr_posts` 2, `pr.post.*` 10, `gate.pr` and `gate.merge` 17, `pr_not_ready` and `pr_not_closed` 5) |
| `workflow` words | 13: 4 GitHub Actions workflow (all on line 1830: the word, the `workflows` key, the API path, "the failed workflow"), 7 "workflow worktree" (stale), 2 `workflow init` (cpd 1) |

The identifier counts come from the regexes of the script that is not kept (the patterns are in the `rg` table of `## Coverage` for `prs`, `pull_request(s)`, `--pr` and the Event kinds; the rest were counted with equivalent patterns, unverified by a second command).
