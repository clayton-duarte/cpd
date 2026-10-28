# Glossary audit: docs/contract.md section 4, Storage

Audit of one part for the Workflow rename (Context records pro-d136, pro-d138). It reads, classifies and reports; it fixes nothing. Line numbers are those of `docs/contract.md` at the base commit of this Attempt (`32bb8cd3`).

## Coverage

The part is `docs/contract.md` lines 1206 to 2419 (1214 lines): the heading `## 4. Storage` at line 1206 through the line before `## 5. Events` (line 2420). The Spec said "about lines 1181 to 2394"; the real range is the one above (checked with `rg -n '^## ' docs/contract.md`).

| Lines | Subsection | Read |
|---|---|---|
| 1206-1207 | `## 4. Storage` heading | in full |
| 1208-1215 | 4.1 The database | in full |
| 1216-1226 | 4.2 Transactions and concurrency | in full |
| 1227-1253 | 4.3 Writes only through cpd: `txns` and `cpd_txn()` (schema check, canonical form, tamper report) | in full |
| 1254-1258 | 4.4 DDL, introduction (the migration list) | in full |
| 1259-1774 | 4.4 the main SQL block (every `CREATE TABLE`, index, trigger, view) | in full, as text; the DDL read as a table of names plus every comment |
| 1775-1826 | 4.4 runner tables, lookup tables, `INSERT` of kinds and `history_tables` | in full |
| 1827-1912 | 4.4 prose: history row shape, Triggers, delete exception, tombstone | in full |
| 1913-2062 | 4.4 SQL: Locked, same-pr, transitions, append-only triggers | in full |
| 2063-2110 | 4.5 normative bullets (N1-N5, R1-R4, K1-K4, A1-A3, O1-O3, W1-W5, C1-C6) | in full |
| 2111-2218 | 4.5 "The legacy record" (migrations 2 to 0014) | in full |
| 2219-2297 | 4.5 named migrations `kind-tables` to `pr-jira` and the last bullet | in full |
| 2298-2352 | 4.6 The Context store (records, retirement, distiller, consumers) | in full |
| 2353-2419 | 4.5 named migrations listed after 4.6 (`plannotator-reviews` to `job-tenancy`) | in full |

Skimmed: nothing in the part. The six `rg` passes below were run over the whole part and every hit was then read in context. Not read, because another part covers it: every other line of `docs/contract.md` (sections 1 to 3 and 5 to 10), including the sections this part cites (`§2.2` "The base PR", "An adopted PR", "Job kinds", `§2.3` fingerprint, `§3.8` "Stacked PRs", `§5.2` Event kinds, `§7.6`, `§8.2`, `§8.3`). Also not read: the code under `core/src/store/` and the migration files; every statement below about what the code does is `unverified` unless a line of the contract says it. Of `CONTEXT.md` only the entries `PR` (phase, step, entity), `Plan`, `Merged`, `PR poll` and the Workflow lines (107-109) were read, to fix the sense codes.

Commands behind every count (all run from the worktree, over `sed -n 1206,2419p docs/contract.md`, 1214 lines):

| Count | Command | Result |
|---|---|---|
| Lines with any pr / pull-request token | `sed -n 1206,2419p docs/contract.md \| rg -c -i '\bprs?\b\|pull[ _-]?requests?\|pr_\|_pr\b\|\bpr-'` | 196 lines |
| Every token classified in `## PR occurrences` | `sed -n 1206,2419p docs/contract.md \| rg -o '(?i:pull[ _-]?requests?)(?:_[a-z_]+)?\|\bPrTitle\b\|\bpr-[a-z][a-z-]*\|\bpr_[a-z_]+\|\b[a-z_]+_pr(?:_[a-z_]+)?\b\|\bprs?(?:_[a-z_]+)?\b\|\bPRs?\b' \| wc -l` | 597 tokens |
| Word `PR` / `PRs` | `sed -n 1206,2419p docs/contract.md \| rg -o '\bPRs?\b' \| wc -l` | 138 |
| Prose "pull request(s)" | `sed -n 1206,2419p docs/contract.md \| rg -o -i 'pull requests?' \| wc -l` | 14 |
| Identifiers `pull_request` / `pull_requests*` | `sed -n 1206,2419p docs/contract.md \| rg -o '\bpull_requests?[a-z_]*' \| wc -l` | 68 |
| Word `Workflow` | `sed -n 1206,2419p docs/contract.md \| rg -n -i 'workflow'` | 1 hit, line 2296 |
| Word `GitHub Actions` | `sed -n 1206,2419p docs/contract.md \| rg -n 'GitHub Actions'` | 0 hits |
| Words `Deploy`, `Mechanical checks`, `Quick check` (capitalised) | `sed -n 1206,2419p docs/contract.md \| rg -n 'Deploy\|Mechanical\|Quick check'` | 0 hits (so no `step` use of PR) |

Classification of the 597 tokens into a sense code was done by rule plus a per-token judgement (a script of mine, not committed because only this file may change). The rules: (R1) the word `PR`/`PRs` and the bare `pr` of a column, flag or command are `object` when they name a Plan's `prs` row, its key, status, needs, repo, base, Lead Branch, Jobs or shape; (R2) `pull request`, `pull_requests*`, `pr_number`, `pr_url`, `pr_opened_at`, `pr_title`, `pr_review_decision`, `pr_checks`, `pr_seen`, `pr_polled_at`, `pr_posts*` and the `pr-` names of migrations about the poll, feedback, base_ref, Inbox kinds, posting and auto-merge are `pull request`; (R3) the Plan phase `pr` is `phase`; (R4) a PR that means "the code change that carries a migration" is `pull request`; (R5) anything undecidable or true of both is `mixed`; (R6) Plan ids and keys that are history (`pr-links`, `pr-post`) are `other`. The `step` code never occurs in this part.

## Terms

One row per term or word that is used in more than one sense, loosely, stale, wrong or colliding. "Where" cites `docs/contract.md:line`.

| Term | Where (path:line) | Sense as used here | Collision | Verdict | Proposed wording |
|---|---|---|---|---|---|
| Workflow | docs/contract.md:2296 | cpd 1's name for Plan: "running Workflows finish on cpd 1" | internal: the new Workflow (Plan > Workflows > Jobs); CONTEXT.md:107-109 retires the old one. The same sentence is at docs/contract.md:4192 (other part) | stale | "running cpd 1 Plans finish on cpd 1" (or "cpd 1 work"); the User decides, since the old word is history |
| PR | docs/contract.md:1235-2416 (597 tokens, see `## PR occurrences`) | `object` 360, `pull request` 193, `phase` 14, `mixed` 24, `other` 6 | internal: Workflow vs pull request vs the old Plan phase | overloaded | `object` becomes Workflow (rule, counts below); `pull request` keeps PR; `phase` stays only as history |
| Plan phase `pr` | docs/contract.md:1256, 2140, 2152, 2153, 2191 | the old phase, now `deploy` | internal: PR the entity; Deploy stage | stale | keep: these lines describe applied migrations (0005, 0008, migration 4) and are history; add one sentence at 2111 saying the record uses the words of its day |
| follow-up PRs | docs/contract.md:2152 | "a Plan takes follow-up PRs after `pr` and `done`": more Workflows, each ends in a pull request | internal | loose | "follow-up Workflows" (history line; see Fix list) |
| adopted PR / adopt | docs/contract.md:1428, 1951, 2290 | a Workflow that took over an existing pull request (`head_ref`, `pr add --adopt`) | internal | loose | "a Workflow that adopts a pull request"; `--adopt` keeps its name |
| need / needs | docs/contract.md:1442, 1443, 1899 | a PR's need (join table `pr_needs`) and a Job's Dependency (`dependencies`) use the same word `needs` | internal: Dependency | loose | "a Workflow's need" and "a Job's Dependency"; table `workflow_needs` |
| merged | docs/contract.md:1387, 1412, 1484, 1534 | four columns, three meanings: `pull_requests.state` (GitHub merged), `prs.status` (the Workflow's pull request merged), `jobs.status` and `attempts.outcome` (fast-forwarded into the Lead Branch, CONTEXT.md `Merged`) | upstream: GitHub merged, git merge | overloaded | always qualify: "Job merged" vs "pull request merged"; Workflow `merged` = its pull request merged |
| open | docs/contract.md:1387, 1412, 1495, 1612 (escalations status), 1682 | `pull_requests.state`, `prs.status`, `jobs.kind` (Open Job), `escalations.status`, Inbox | upstream: GitHub open | overloaded | keep; qualify in prose (Open Job, open pull request, open Workflow) |
| draft | docs/contract.md:1386, 1394, 1484, 1899 | `jobs.status` draft (a Job with no Spec yet) vs `pull_requests.draft` and `merge_state` `draft` (a GitHub draft pull request) | upstream: GitHub draft | overloaded | "draft Job" vs "draft pull request"; never bare "draft" |
| checks | docs/contract.md:1235, 1389, 1256 | `pull_requests.checks` (GitHub CI checks); the schema check; SQL `CHECK`; "kind checks" | upstream: GitHub checks; internal: Quick check, Mechanical checks | overloaded | "CI checks" for the pull request; "schema check" for 4.3; `CHECK` always in caps and code |
| review | docs/contract.md:1454, 1475, 1389, 1682 | `jobs.review` (a Review Job flag), `plan_reviews` (Plan review), `review_decision` (GitHub), Inbox `review-waiting` / `review-requested` | upstream: GitHub review | overloaded | "Plan review", "Review Job", "pull request review"; never bare "review" |
| feedback | docs/contract.md:1398, 1460 | `pull_requests.feedback` (Feedback items of a pull request) vs `plan_reviews.feedback` (a Plan review's text) | internal: Feedback item | loose | `plan_reviews.feedback` is "review comment"; keep Feedback item for the pull request |
| status / state | docs/contract.md:1387, 1412, 1484, 1640 | `status` on jobs, prs, attempts, plan_reviews, escalations; `state` on sessions and `pull_requests`; `merge_state` | upstream: GitHub `state`, `mergeStateStatus` | loose | pre-existing; no change needed for the rename |
| base | docs/contract.md:1399, 1411, 1936, 2082 | `prs.base` (branch the pull request targets), `base_ref` (GitHub's), "base PR", "merges the base" (git) | upstream: GitHub baseRef; git base | overloaded | "base branch" for the branch; "base Workflow" for the stacked one |
| owner | docs/contract.md:1291, 1382, 1581, 1874-1884, 2240 | `contexts.owner` (GitHub login, policy key), GitHub `owner/name`, pool `owner:<login>`, `<owner>` of a row in the delete exception | upstream: GitHub owner; internal: Known owner | overloaded | in 1874-1884 write "owning row" or "owner row" in code font only; keep `owner` for the GitHub login |
| Locked / lock | docs/contract.md:1221, 1252, 1495, 1924, 1934, 1938 | Locked Job/field, the write lock, `store.lock`, Plan `locked_at`, trigger `prs_locked`, error `pr_locked` | upstream: `flock` | overloaded | keep Locked for Plan/Job fields; "write lock" and "setup lock" for files |
| dispatch | docs/contract.md:1367, 1412, 1934, 2014 | `plans.phase` dispatch and `prs.status` dispatch (and `waiting>dispatch`) are the same word for two state machines | internal: Dispatch | overloaded | pre-existing; after the rename it reads "Workflow status `dispatch`" |
| waiting | docs/contract.md:1412, 1640, 1682, 2222 | `prs.status` waiting, `sessions.state` waiting, Inbox `review-waiting`, migration `waiting-state` | internal | overloaded | pre-existing |
| blocked | docs/contract.md:1394, 1640 | `pull_requests.merge_state` blocked (GitHub) vs `sessions.state` blocked | upstream: GitHub BLOCKED | overloaded | qualify in prose |
| deploy | docs/contract.md:1367, 1682, 2410 | Plan phase `deploy`, Inbox kind `deploy`, migration `deploy-kind` | internal: Deploy stage | loose | the Inbox kind's meaning is not given in this part (unverified); say what it announces |
| context | docs/contract.md:1364, 1480, 1924 | `jobs.context` (a Spec field) vs `plans.context` (the Context entity) vs `context_records` | internal: Context | overloaded | pre-existing; Spec field could be "background" (not proposed here; the User decides) |
| tier | docs/contract.md:1478, 2397, 2402 | `jobs.tier`, `sessions.tier` (model Tier) vs "blue Inbox tier" and migrations `blue-tier`, `cyan-tier` (Inbox urgency) | internal: Tier | overloaded | "urgency" for the Inbox colour; migrations are history |
| queue / queued | docs/contract.md:1740, 1927, 2019, 2082 | `jobs.status` queued, the Slack reaction queue (`queued_at`), "queues the PR's update Job" | internal | loose | pre-existing |
| archive | docs/contract.md:1372, 2130, 2306 | `archived_at` (soft archive), the folder `archive/contexts/`, migration `0003-archive` | upstream: git archive | loose | pre-existing |
| history | docs/contract.md:1827, 1829 | the `history` table (row changes) | upstream: git history | loose | pre-existing; "row history" when git is also in view |
| stage / stages (verb) | docs/contract.md:1235 | "stages the file" (git index) | upstream: git stage; internal: Plan stage | ok | none; the verb is git's |
| spec | docs/contract.md:2397 | "before this spec was approved": this contract, not a Job's Spec | internal: Spec | loose | "before this contract section was approved" |
| plan (verb) | docs/contract.md:2314 | "asking it to plan a docs PR" | internal: Plan | loose | "to add a docs Workflow"; or leave |
| finding | docs/contract.md:2219, 2395 | "Plan quality findings sql-F4" (audit findings); "The findiing of hs-n427" is a typo | internal: audit finding | loose | fix the typo (2395); no rename effect |
| record | docs/contract.md:1221, 1250, 1827 | "records itself", "records names" (verb), Context record, history row | internal: Context record | loose | pre-existing |
| Summary | the rest | Plan, Context, Context record, Job, Job kind, Open Job, Update Job, Spec, Review Job, Dependency, Attempt, Ledger, Session, Event, Inbox, Escalation, Gate, Lead Branch, Tier, Policy key, Manager, Brief and the other CONTEXT.md terms are used in one sense or are consistently qualified in this part | | ok | none |

## PR occurrences

Every row below states its Where range, a count, one sense code, a quote of at most 15 words with the line it is found at, and what it becomes. The first table covers `object` and `pull request` by section (a mechanical rewrite for `object`). The second table lists every `mixed`, `phase` and `other` use individually. Counts of both tables add up to the 597 tokens.

### Mechanical groups (`object` and `pull request`)

After the rename, `object` = Workflow: prose "PR" becomes "Workflow"; identifiers follow the proposed names `prs` to `workflows`, `pr_needs` to `workflow_needs`, `jobs.pr` to `jobs.workflow`, `pr_locked` to `workflow_locked`, `needs_other_pr` to `needs_other_workflow` (names proposed, the User decides). `pull request` keeps its words.

| Where (section, lines) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| 4.3 schema check (1227-1253) | 1 | object | 1243: "`dependencies`, `pr_needs`; `pull_requests` has the second only" | `workflow_needs` |
| 4.3 schema check (1227-1253) | 2 | pull request | 1235: "A migration's PR updates the constant in the same change" | unchanged |
| 4.4 intro (1254-1258) | 6 | object | 1256: "created `prs` and `pr_needs` (R34)" | `workflows`, `workflow_needs` |
| 4.4 intro (1254-1258) | 11 | pull request | 1256: "`0006-pr-poller` created `pull_requests`" | unchanged (migration names and table) |
| 4.4 DDL incident (1259-1378) | 5 | object | 1303: "the auto-end PR key, null unless `--until-pr`" | "auto-end Workflow key", `until_workflow`, `--until-workflow` |
| 4.4 DDL `pull_requests` (1379-1405) | 10 | pull request | 1379: "Pull requests (the PR poller, hs-d96" | unchanged |
| 4.4 DDL `prs` (1406-1441) | 21 | object | 1426: "a PR that existed before it has none" | "a Workflow that existed before it has none" |
| 4.4 DDL `prs` (1406-1441) | 13 | pull request | 1411: "the branch the pull request targets" | unchanged (`pr_number`, `pr_url`, `pr_opened_at`, `pull_request` describe the pull request) |
| 4.4 DDL `pr_needs` (1442-1452) | 10 | object | 1442: "keep every need on an existing PR" | `workflow_needs`, column `workflow` |
| 4.4 DDL jobs and others (1453-1717) | 4 | object | 1474: "R34: the Job's PR (A7)" | `jobs.workflow` |
| 4.4 DDL jobs and others (1453-1717) | 3 | pull request | 1682: "'pr-failing', 'pr-changes', 'pr-ready'" | unchanged (Inbox kinds about pull requests) |
| 4.4 DDL `pr_posts`, `slack_reactions` (1718-1774) | 8 | object | 1731: "FOREIGN KEY (plan, pr) REFERENCES prs(plan, key)," | `pr_posts.workflow`, `slack_reactions.workflow`, `workflows(plan, key)` |
| 4.4 DDL `pr_posts`, `slack_reactions` (1718-1774) | 3 | pull request | 1719: "CREATE TABLE pr_posts (" | unchanged (the table records the Slack post of a pull request) |
| 4.4 lookup tables (1775-1826) | 7 | object | 1803: "('pr-added'), ('pr-edited'), ('pr-cancelled'), ('pr-started')," | decision needed: see Fix list (Event kinds cannot be renamed in place) |
| 4.4 lookup tables (1775-1826) | 3 | pull request | 1802: "('all-merged'), ('pr-feedback')," | unchanged |
| 4.4 prose triggers (1827-1912) | 19 | object | 1899: "a PR's need may be deleted while its Plan is in" | "a Workflow's need ..."; `workflow_needs_*` triggers |
| 4.4 prose triggers (1827-1912) | 13 | pull request | 1899: "A pull request row stays: the poll does not delete rows" | unchanged |
| 4.4 SQL triggers (1913-2062) | 53 | object | 1934: "a PR's identity, repo and base are Locked once its Plan is locked" | "a Workflow's identity ..."; `workflows_locked`, `workflows_status`, `workflow_locked` code |
| 4.5 normative (2063-2110) | 16 | pull request | 2081: "When two PRs both add breaking migrations" | unchanged (the PR is the code change) |
| 4.5 normative (2063-2110) | 2 | object | 2105: "one per Context and PR" | "one per Context and Workflow" |
| 4.5 legacy record, migrations 2-5 (2111-2156) | 24 | object | 2133: "fill `prs` with **one PR per Plan**" | history: "one Workflow per Plan", see Fix list |
| 4.5 legacy record, migrations 2-5 (2111-2156) | 22 | pull request | 2121: "A Plan that has a PR when the migration runs" | unchanged (columns `pr_title`, `pr_checks`, ... held the pull request) |
| 4.5 migration 6 (2157-2190) | 30 | object | 2185: "`pr_needs` and `jobs` keep referencing `prs(plan, key)`" | history; `workflows` |
| 4.5 migration 6 (2157-2190) | 35 | pull request | 2157: "**Migration 6** (the PR poller, hs-d96, D5, D6)" | unchanged |
| 4.5 migration 0008 (2191) | 2 | object | 2191: "the PR statuses and `data.pr` are unchanged" | "the Workflow statuses and `data.workflow`", history |
| 4.5 migrations 0009-0014 (2192-2218) | 25 | pull request | 2200: "the PR poll's mergeable state" | unchanged |
| 4.5 migrations 0009-0014 (2192-2218) | 1 | object | 2217: "`jobs_pr_dependencies`" | `jobs_workflow_dependencies` |
| 4.5 `kind-tables` to `pr-description` (2219-2239) | 20 | object | 2236: "existing PRs keep a null `title`" | "existing Workflows keep a null `title`" |
| 4.5 `kind-tables` to `pr-description` (2219-2239) | 1 | pull request | 2235: "in the same PR (§5.3)" | unchanged |
| 4.5 policy keys (2240-2258) | 14 | object | 2246: "a PR's name from its own pull request's owner/name" | "a Workflow's name from its own pull request's owner/name" |
| 4.5 policy keys (2240-2258) | 12 | pull request | 2249: "the pull requests of its PRs" | "the pull requests of its Workflows" (only the second noun changes) |
| 4.5 `pr-cancel-*`, `stacked-prs`, `local-needs` (2259-2272) | 68 | object | 2263: "start a PR whose need is a PR of another Plan" | "a Workflow whose need is a Workflow of another Plan" |
| 4.5 `pr-cancel-*`, `stacked-prs`, `local-needs` (2259-2272) | 1 | pull request | 2260: "ends an `open` PR whose pull request is closed" | unchanged (the word "pull request") |
| 4.5 `job-kinds` to `pr-jira` (2273-2297) | 46 | object | 2290: "every existing PR keeps a null `head_ref` (a cpd PR)" | "every existing Workflow ... (a cpd Workflow)" |
| 4.5 `job-kinds` to `pr-jira` (2273-2297) | 13 | pull request | 2294: "section 2.2 Pull request, section 4.4, section 7.6" | unchanged |
| 4.6 Context store (2298-2352) | 4 | object | 2333: "for each distinct `prs.repo` of the Context's Plans' PRs" | `workflows.repo`, "Plans' Workflows" |
| 4.5 later migrations (2353-2419) | 15 | object | 2414: "whose PR is `merged` (in status `merged`)" | "whose Workflow is `merged`" |
| 4.5 later migrations (2353-2419) | 10 | pull request | 2414: "`ts` = `pr_posts.ts`" | unchanged |

Subtotals of this table: `object` 360, `pull request` 193. Sections 4.1 and 4.2 (1206-1226) have no occurrence, so they have no row.

### Individually listed: `mixed`, `phase`, `other` (44 uses)

| Where | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| docs/contract.md:1256 | 1 | phase | 1256: "renamed the Plan phase `pr` to `deploy`" | history of applied migration 0008: keep |
| docs/contract.md:1256 | 1 | other | 1256: "Plan pr-post" | a Plan id (history): keep |
| docs/contract.md:1418 | 1 | mixed | 1418: "set by pr open" | `pr open` acts on a Workflow and opens its pull request; the User picks the command noun |
| docs/contract.md:1426 | 1 | mixed | 1426: "(`PrTitle`)" | a type named for the PR title, used for a Workflow title here and a Plan review title at 1463; propose `Title` |
| docs/contract.md:1428 | 1 | mixed | 1428: "(§2.2 "An adopted PR")" | the Workflow adopts a pull request; follow the §2.2 heading |
| docs/contract.md:1429 | 1 | mixed | 1429: "the PR's description, a Markdown body" | stored on the Workflow, becomes the pull request body: "the Workflow's description" |
| docs/contract.md:1463 | 1 | mixed | 1463: "(`PrTitle`)" | as 1426 |
| docs/contract.md:1718 | 1 | other | 1718: "Plan pr-post" | a Plan id (history): keep |
| docs/contract.md:1803 | 1 | mixed | 1803: "('pr-opened')" | the Workflow's pull request was opened; Event kind decision, see Fix list |
| docs/contract.md:1803 | 1 | mixed | 1803: "('pr-merged')" | the Workflow's pull request merged; same decision |
| docs/contract.md:1951 | 1 | mixed | 1951: "written once by `pr add --adopt` and never changed" | the command adds a Workflow that adopts a pull request |
| docs/contract.md:2079 | 1 | mixed | 2079: "in the PR's open and update Jobs" | the Jobs belong to the Workflow: "the Workflow's open and update Jobs" |
| docs/contract.md:2082 | 1 | mixed | 2082: "queues the PR's update Job" | "queues the Workflow's update Job" (the conflict is on the pull request) |
| docs/contract.md:2082 | 1 | mixed | 2082: "`pr rebase` merges the base" | acts on the Workflow's Lead Branch for its pull request; command noun decision |
| docs/contract.md:2131 | 1 | mixed | 2131: "adds the six `pr-*` Event kinds" | history; Event kind decision |
| docs/contract.md:2140 | 1 | phase | 2140: "`pr` → `open`" | history: the old phase `pr` mapped to Workflow status `open` |
| docs/contract.md:2152 | 1 | mixed | 2152: "a Plan takes follow-up PRs after `pr` and `done`" | "follow-up Workflows"; history |
| docs/contract.md:2152 | 1 | phase | 2152: "after `pr` and `done`" | history: keep |
| docs/contract.md:2153 | 1 | phase | 2153: "`plans_phase` gains `pr>dispatch` and `done>dispatch`" | history: keep |
| docs/contract.md:2191 | 1 | phase | 2191: "the Plan phase `pr` is called `deploy`" | history: keep |
| docs/contract.md:2191 | 1 | phase | 2191: "`phase = 'pr'` written `'deploy'`" | history: keep |
| docs/contract.md:2191 | 1 | phase | 2191: "in place of `dispatch>pr`" | history: keep |
| docs/contract.md:2191 | 1 | phase | 2191: "`pr>done` and `pr>dispatch`" | history (the token `pr>done`): keep |
| docs/contract.md:2191 | 1 | phase | 2191: "`pr>done` and `pr>dispatch`" | history (the token `pr>dispatch`): keep |
| docs/contract.md:2191 | 1 | phase | 2191: "that read `"pr"` are rewritten to `"deploy"`" | history: keep |
| docs/contract.md:2191 | 1 | mixed | 2191: "the `cpd pr …` command names" | `cpd pr` holds Workflow commands (`add`, `edit`) and pull request commands (`poll`, `post`); noun decision |
| docs/contract.md:2191 | 1 | phase | 2191: "Events with `pr` in `from` and `to`" | history: keep |
| docs/contract.md:2191 | 1 | phase | 2191: "where it held `pr` and no `pr` phase anywhere" | history (the first token): keep |
| docs/contract.md:2191 | 1 | phase | 2191: "where it held `pr` and no `pr` phase anywhere" | history (the second token): keep |
| docs/contract.md:2191 | 1 | phase | 2191: "`deploy>done` is accepted and `pr>done` refused" | history: keep |
| docs/contract.md:2222 | 1 | mixed | 2222: "after `event-pr-base-changed`" | the base of a Workflow follows its pull request's base; Event kind decision |
| docs/contract.md:2236 | 1 | mixed | 2236: "`pr open` refuses them (`pr_title_required`, §7.6)" | as 1418; the error code is about the title of the pull request to be opened |
| docs/contract.md:2238 | 1 | mixed | 2238: "`pr open` refuses them (`description_required`, §7.6)" | as 1418 |
| docs/contract.md:2271 | 1 | mixed | 2271: "**`event-pr-base-changed`** (§5.2)" | migration name is history; the Event kind is a decision |
| docs/contract.md:2271 | 1 | mixed | 2271: "VALUES ('pr-base-changed');" | Event kind decision |
| docs/contract.md:2276 | 1 | mixed | 2276: "none of `pr open`'s rows" | as 1418 |
| docs/contract.md:2289 | 1 | mixed | 2289: "**The config key `pr_open_escalate_minutes` goes with it**" | a removed config key (§8.2, other part); history: keep, check §8.2 |
| docs/contract.md:2291 | 1 | mixed | 2291: "reads the feedback of every PR a Plan owns once" | "every Workflow's pull request": the feedback is the pull request's, the owner is the Workflow |
| docs/contract.md:2295 | 1 | mixed | 2295: "only a PR whose owner's policy sets `pr.jira_required`" | policy key namespace `pr` (policy file, unverified) covers pull request rules too; the User decides |
| docs/contract.md:2314 | 1 | mixed | 2314: "asking it to plan a docs PR" | the Manager plans a docs Workflow that ends in a pull request |
| docs/contract.md:2393 | 1 | other | 2393: "pr-links reached done with a pending r4" | a Plan id (history): keep |
| docs/contract.md:2395 | 1 | other | 2395: "r4 of pr-links was requested on 2026-10-19" | a Plan id (history): keep |
| docs/contract.md:2395 | 1 | other | 2395: "pr-links r4, quality r4 and r5" | a Plan id (history): keep |
| docs/contract.md:2412 | 1 | other | 2412: "Plan pr-post" | a Plan id (history): keep |

### Schema objects that carry `pr` or `pull_request`

Views: none carries pr (the only view is `job_tenants`, docs/contract.md:1593). A row's Sense is the sense of the name.

| Kind | Name | Where | Sense | Why |
|---|---|---|---|---|
| table | `pull_requests` | docs/contract.md:1380 | pull request | one row per GitHub pull request: number, url, state, checks, review decision |
| table | `prs` | docs/contract.md:1407 | object | one row per Workflow: key, repo, base, status, Lead Branch |
| table | `pr_needs` | docs/contract.md:1444 | object | needs between `prs` rows |
| table | `pr_posts` | docs/contract.md:1719 | pull request | the Slack post of a pull request's link, its reaction and auto-merge; keyed `(plan, pr)` with a foreign key to `prs`, so its `pr` column is `object` |
| column | `prs.pr_number`, `pr_url`, `pr_opened_at` | docs/contract.md:1415-1417 | pull request | the number, URL and opening time of the pull request, kept on the Workflow row |
| column | `prs.pull_request` | docs/contract.md:1418 | pull request | foreign key to `pull_requests(id)`; duplicates `pr_number` and `pr_url` (loose, see Fix list) |
| column | `jobs.pr` | docs/contract.md:1474 | object | the Job's Workflow |
| column | `pr_needs.pr` | docs/contract.md:1446 | object | the Workflow that needs |
| column | `pr_posts.pr`, `slack_reactions.pr` | docs/contract.md:1721, 1743 | object | key of a Workflow |
| column | `incident.until_pr` | docs/contract.md:1303 | object | the auto-end Workflow key |
| triggers, hand-written, name carries pr (11) | `pull_requests_owned`, `pull_requests_hist_del` | docs/contract.md:1904, 1901 | pull request | guard and history of the `pull_requests` table |
| | `prs_locked`, `prs_shape_fixed`, `prs_head_ref_fixed`, `prs_status` | docs/contract.md:1938, 1948, 1952, 2012 | object | guards of a Workflow row |
| | `pr_needs_locked`, `pr_needs_no_delete`, `pr_needs_hist_del` | docs/contract.md:1958, 1899 | object | guards of the need table |
| | `dependencies_same_pr`, `jobs_pr_dependencies` | docs/contract.md:1990, 1995 | object | Jobs of one Workflow |
| triggers, generated, name carries pr (12) | `prs_hist_ins`, `prs_hist_upd`, `prs_hist_del`, `prs_no_delete` | docs/contract.md:1829 | object | derived from `history_tables` row `('prs', 1)` at 1818 |
| | `pr_needs_hist_ins`, `pr_needs_hist_upd` | docs/contract.md:1829 | object | row `('pr_needs', 0)` at 1818 |
| | `pull_requests_hist_ins`, `pull_requests_hist_upd` | docs/contract.md:1829 | pull request | row `('pull_requests', 0)` at 1818 |
| | `pr_posts_hist_ins`, `_hist_upd`, `_hist_del`, `_no_delete` | docs/contract.md:1829, 2412 | pull request | `pr_posts` is in `history_tables` with `no_delete` 1 (2412) |
| trigger with `pr` in its body only | `jobs_locked` | docs/contract.md:1925, 1931 | object | column list and `NEW.pr IS OLD.pr` |
| indexes (1) | `prs_head_ref_live` | docs/contract.md:1956 | object | on `prs (repo, head_ref)` |
| indexes named but not in the DDL (2) | `prs_unique`, `prs_live` | docs/contract.md:2292 | object | stale: the DDL at 1406-1441 has neither (Fix list) |
| migration names, `pull request` (7) | `0006-pr-poller`, `0011-pr-mergeable`, `0013-pr-inbox`, `pr-posts`, `pr-auto-merge`, `pr-comments`, `pr-base` | docs/contract.md:2068, 1718, 2291, 2294 | pull request | each is about the poll, GitHub fields, Inbox kinds or Slack posts |
| migration names, `object` (6) | `pr-title`, `pr-description`, `pr-jira`, `pr-adopt`, `pr-cancel-started`, `pr-cancel-open` | docs/contract.md:2236, 2238, 2295, 2290, 2259, 2260 | object | fields and transitions of a Workflow row |
| migration names, `object` (1) and `mixed` (1) | `stacked-prs`; `event-pr-base-changed` | docs/contract.md:2263; 2271 | object; mixed | stacked Workflows; the base Event |
| Event kind (1) | `pr-feedback` | docs/contract.md:1802 | pull request | sent for a pull request that needs attention |
| Event kinds (4) | `pr-added`, `pr-edited`, `pr-cancelled`, `pr-started` | docs/contract.md:1803 | object | the life of a Workflow |
| Event kinds (3) | `pr-opened`, `pr-merged`, `pr-base-changed` | docs/contract.md:1803, 2271 | mixed | a Workflow's pull request is opened, merged or retargeted |
| Inbox kinds (3) | `pr-failing`, `pr-changes`, `pr-ready` | docs/contract.md:1682 | pull request | state of a pull request |
| error codes (2) | `pr_locked`, `needs_other_pr` | docs/contract.md:1945, 1993 | object | refusals of a Workflow row or of its Jobs |
| error code (1) | `pr_title_required` | docs/contract.md:2236 | mixed | a Workflow without the title its pull request needs |

## Fix list

Mechanical rule: every `object` row of the first table above is a Workflow rewrite; counts per section are in that table (360 in all). Only what needs a judgement is listed here.

| path:line | what to change | why |
|---|---|---|
| docs/contract.md:2296 | decide the wording of "running Workflows finish on cpd 1" (and the same at 4192 in another part) | the old Workflow (cpd 1's name for Plan) collides with the new one; `stale` |
| docs/contract.md:2111 | decide whether the legacy record (2111-2218, about 140 tokens: migrations 2 to 0014) keeps its words with one added sentence, or is reworded | it describes applied migrations in the words of its day (PR phase, PR = Workflow); History keeps the old words, so proposed: keep, add a sentence |
| docs/contract.md:1256, 2140, 2152, 2153, 2191 | keep the `phase` uses (14); do not reword | applied migrations 0005 and 0008 and migration 4's status map: history |
| docs/contract.md:1418, 2236, 2238, 2276 | decide the command noun of `pr open` (and `pr edit`, `pr describe`, `pr rebase` at 2082, 2236, 2238, `cpd pr …` at 2191) | the command acts on a Workflow and opens its pull request: `mixed`; `pr poll`, `pr post`, `pr reply`, `pr merge` stay (pull request) |
| docs/contract.md:2079, 2082 | "the PR's open and update Jobs" and "the PR's update Job" become the Workflow's | the Jobs belong to the Workflow; the surrounding "PR" (the code change) stays |
| docs/contract.md:1426, 1463 | rename the type `PrTitle` | one type serves a Workflow title (72 chars) and a Plan review title; neither is a pull request |
| docs/contract.md:1428, 1951, 2290 | one wording for "adopted PR" and `pr add --adopt` | a Workflow adopts an existing pull request: `mixed` |
| docs/contract.md:1429, 2238, 1426, 2236 | say "the Workflow's description/title (it becomes the pull request's)" | stored on the Workflow, published to the pull request |
| docs/contract.md:1415-1418 | note that `pr_number`, `pr_url`, `pr_opened_at` and `pull_request` are the pull request's and stay; consider dropping the duplicate of `pull_request` | the same fact is stored twice on one row (`pr_number`/`pr_url` and the `pull_requests` row); loose, not a rename need |
| docs/contract.md:1802-1803, 2131, 2222, 2271 | decide the Event kinds `pr-added`, `pr-edited`, `pr-cancelled`, `pr-started`, `pr-opened`, `pr-merged`, `pr-base-changed`: new `workflow-*` kinds with the old ones kept for history, or keep | `events.kind` references `event_kinds(name)` (1665), and `events_no_update` (2054) forbids changing old Events, so a row cannot be renamed while Events use it; "no aliases" and "history keeps its words" pull against each other; each new kind raises `PROTOCOL` (2076, 2271) and the adapter's `KINDS` (§5.3, other part); Event `data.pr` keys (2191) likewise stay in old Events |
| docs/contract.md:1818, 1829, 1878, 1897 | the table rename must also update `history_tables` rows `('prs', 1)` and `('pr_needs', 0)` and the lists at 1829, 1878 and 1897 | the generator reads `history_tables`; the schema check verifies `T_hist_ins` and the others per row (1243); `kind-tables` is frozen (2071), so an `UPDATE` in a new migration is the only way, and `UPDATE` of existing rows is breaking (2078) |
| docs/contract.md:2077 | decide rename-in-place versus copy-and-drop for `prs` and `pr_needs` | K4 says dropping a table listed in `history_tables` is "not supported until needed"; a copy-rename drops one. `ALTER TABLE RENAME` avoids that, but with `legacy_alter_table = ON` (2098, 2132, 2158) SQLite's rewriting of trigger bodies that name the renamed table is `unverified`; hand-written triggers that name `prs` (`prs_locked` at 1944, `pull_requests_owned` at 1905, `pr_needs_locked` at 1960) must be dropped and created again |
| docs/contract.md:1243, 1244 | after a rename the old generated trigger names (`prs_hist_*`, `prs_no_delete`) must be dropped by the migration | item 5 of the canonical form: a trigger with any other name is an object of item 4, hashed as text, so a stale one changes `SCHEMA_HASH` and also fails check (c) at 1248; "A migration never creates a generated trigger (it may drop them)" (1829) allows the drop |
| docs/contract.md:1237-1245, 1235 | the canonical form needs no change; `SCHEMA_HASH` is re-pinned by `scripts/repin.sh` | table entries are `["table", name, columns, constraints, options]` with columns sorted by lower-cased key (1239): a renamed table or column (`jobs.pr`) changes name, sort position and constraint text (`FOREIGN KEY (plan, pr) REFERENCES prs(plan, key)`) of those entries only |
| docs/contract.md:2078, 2081, 2083 | the rename migration is `breaking after <n>` (a `RENAME`, trigger recreations and `UPDATE history_tables` are not among the additive statements of A1) | O3 makes it a barrier; every installed binary that knows `prs` is refused with `store_too_new` (2083), so the cutover is all binaries at once; names `0006-pr-poller`, `pr-title` and the others are applied names in `schema_migrations` and stay (N5, 2071) |
| docs/contract.md:1925, 1931, 1995-1996, 1990, 1938, 1948, 1952, 1958, 2012 | recreate every hand-written trigger whose column list or body names `pr` or `prs`: `jobs_locked` (`UPDATE OF ... pr ...`), `jobs_pr_dependencies` (`UPDATE OF pr`), `dependencies_same_pr`, `prs_locked`, `prs_shape_fixed`, `prs_head_ref_fixed`, `prs_status`, `pr_needs_locked`, `pr_needs_no_delete`, `pr_needs_hist_del` | locked fields: `jobs.pr` is in the Locked column list (1925) and the trigger text is part of the schema hash; whether `RENAME COLUMN` rewrites an `UPDATE OF` list under `legacy_alter_table` is `unverified` |
| docs/contract.md:1945, 1949, 1953, 1961, 1899, 1993, 1997 | decide the error codes `pr_locked` and `needs_other_pr` (and `pr_title_required` at 2236) | they are strings inside `RAISE(ABORT, ...)` of triggers, so changing them changes the trigger text and the hash, and the CLI's code table (other part) |
| docs/contract.md:1870, 1878 | generated `T_no_delete` raises `append_only: <table>` (1870), so the message of `prs` becomes `append_only: workflows`; the delete-exception table at 1878 lists `prs`, `pr_needs`, `pr_posts` | the table names appear in error text and in the exception table; update both |
| docs/contract.md:1252, 1495, 1924 | check that the Plan fingerprint (§2.3, other part) does not contain the key name `pr` of `jobs.pr` | the tamper report recomputes every Locked Job's fingerprint and compares it to the stored one (1252); if the canonical text holds the field name, renaming it would flag every Locked Job as tampered: `unverified`, needs the §2.3 audit |
| docs/contract.md:1827, 1829 | add a rule for readers of `history`: old rows have `tbl` `prs`/`pr_needs` and JSON keys `pr`, new rows `workflows`/`workflow` | `history` is append-only (2053) and "the history stays replayable" (1827); replay by table name breaks at the rename |
| docs/contract.md:1303, 1302 | `incident.until_pr`, `until_plan` and `--until-pr` become `until_workflow` and `--until-workflow` | an `object` column and flag; a column rename (breaking) in the same migration |
| docs/contract.md:1721, 1743, 1731 | rename `pr_posts.pr` and `slack_reactions.pr`; keep the table name `pr_posts` | the column is the Workflow key; the table records the post of a pull request (`pull request`) |
| docs/contract.md:1428, 1936, 2263, 2290 | update the cross-references `§2.2 "An adopted PR"`, `§2.2 "The base PR"`, `§3.8 "Stacked PRs"` after the other parts rename those headings | cross-part links |
| docs/contract.md:2295 | decide the policy key namespace `pr.jira_required` | the `pr` namespace of the policy file also holds pull request rules (merge, push); `mixed` |
| docs/contract.md:2314 | "asking it to plan a docs PR" becomes "a docs Workflow" | `mixed`; also a loose use of the verb "plan" |
| docs/contract.md:2289 | check the removed config key `pr_open_escalate_minutes` in §8.2 | named after `pr open`; history, unverified what it governed |
| docs/contract.md:2258 | keep as the model sentence: "A PR with no pull request yet" | already separates the two senses; after the rename "A Workflow with no pull request yet" |
| docs/contract.md:1256, 2295, 2404, 2410 | one statement of the newest migration | 1256 says `lead-branch-name` is the latest; 2295 says `pr-jira`; 2404 says `20261102T1100-pr-base`; 2410 says `20261102T0900-deploy-kind` is "applied after every other"; 2416 lists `20261102T1300-slack-reaction-refusal`; `wrong` (stale) |
| docs/contract.md:2292 | remove or correct `prs_unique`, `prs_live` and "migration 4.5" | `wrong`: the DDL (1406-1441) has no index of either name (only `prs_head_ref_live`, 1956), and there is no migration 4.5 |
| docs/contract.md:2389 | check `plan_reviews_number` and `plan_reviews_pending` | `wrong` (not a rename matter): the DDL of `plan_reviews` (1454-1468) has neither; reported only because it was seen |
| docs/contract.md:2395 | fix "findiing" | typo |
| docs/contract.md:2397 | "this spec was approved" becomes "this section" | `spec` collides with the Job's Spec |
| docs/contract.md:2397, 2402 | `blue-tier` / `cyan-tier`: say "urgency", not "tier" | collides with Tier (model Tier); the migration names are history |

## Counts

| What | Number |
|---|---|
| Files audited | 1 (`docs/contract.md`, lines 1206-2419) |
| Lines in the part | 1214 |
| Lines read in full | 1214 |
| Lines skimmed / not read | 0 / 0 (cross-referenced sections and code: not read, listed in Coverage) |
| Lines with a pr / pull-request token | 196 |
| Tokens classified | 597 |
| `object` | 360 |
| `pull request` | 193 |
| `phase` | 14 |
| `step` | 0 |
| `mixed` | 24 |
| `other` | 6 |
| Word `PR` / `PRs` | 138 |
| Prose "pull request(s)" | 14 |
| Identifiers `pull_request*` | 68 |
| Rows in `## Terms` | 31 |
| Rows in `## PR occurrences` (mechanical groups / individual / schema objects) | 38 / 44 / 30 |
| Rows in `## Fix list` | 33 |
| Schema objects that carry pr: tables 4, hand-written triggers 11, generated triggers 12, views 0, indexes 1 (+2 stale), migration names 15, Event kinds 8, Inbox kinds 3, error codes 3 | |
