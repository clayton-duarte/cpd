# Glossary audit: how the words are really used

Part: Job `aud-usage` (Attempt `aud-usage-a1`) of Plan `glassary`. Read-only audit: nothing was fixed, renamed or reworded; the User decides every contested term. Sense codes for 'PR' are exactly `object`, `pull request`, `phase`, `step`, `mixed`, `other` (Plan brief). Dates are the UTC dates stored in the record headers, as written there.

Paths: `ctx` = `/tmp/cpd-glassary/context-cpd.md`, `titles` = `/tmp/cpd-glassary/titles.md`. Every citation is `path:line`; a record id (`pro-d85`) names the record at that line. A record's author is not stored in the file: unless the text names one ("Manager", "Lead"), the author is "agent (Manager or Lead; unverified which)", and a decision that says "(User, date)" is an agent's record of what the User decided, not the User's own sentence unless the text quotes it.

## Coverage

What I read:

- `ctx` (428 lines, 152 KB): read in full, in chunks of 60 to 140 lines (`sed -n 'a,bp'`), lines 1-428. Lines up to 2,129 characters were shown cut at 700-1,200 characters in the chunk views, so the tails of the longest records (for example pro-d18, pro-d60, pro-d64, pro-d68, pro-d69, pro-d100, pro-d106, pro-d126, pro-d131) I read only through `rg -o` hits, not line by line: unverified for words that appear only in those tails.
- `titles` (386 lines): read through `rg` hits for 'PR', 'pull request', review, workflow, deploy, draft, phase; lines 1-60 and the `## Plan` header lines read directly. I did not read all 290 `- PR <key>` lines one by one.
- `CONTEXT.md` of this worktree: only used to answer 'does CONTEXT.md define this term' with `rg -c -i -F` (section `Words agents invented`) and to name what it says for PR (section `PR in practice`). Auditing CONTEXT.md is another part's job.

What I did not read: any file outside the two inputs and `CONTEXT.md`; the Context records of other Contexts (only their Plan and PR titles, via `titles`); source code. No `gh`, no network.

Method limits:

- All counts are `rg -c`, which counts matching LINES, not occurrences. One record is one line in `ctx`, so a count is "records and lines that contain it".
- `ctx` has 265 `pro-d`/`pro-n` records (`rg -c '^- pro-[dn][0-9]+'`), 69 Plans (`rg -c '^- pro-p'`) and 69 briefs (`rg -c 'brief pro-b'`). `titles` has 97 Plans (`rg -c '^## Plan'`) and 290 PR lines (`rg -c '^- PR '`). Per the `titles` header rule, every `- PR <key>` line is the cpd object whatever the word says; those 290 lines are cpd-generated text, not agents' prose.
- Sense codes below are my reading of one sentence each; where two senses fit I wrote `mixed` and quoted why.
- Quotes are at most 15 words per cell and were machine-checked as exact substrings of the cited file (`rg -F`).

The `rg` command behind every count is in the `Counts` section and in the Count column of `PR in practice`; `Confusions` counts cite the matching row of `Counts`.

## Confusions

Ranked by harm (a wrong action or a wrong reading by an agent), then frequency. Count = lines matching the `Counts` row named in brackets, in `ctx` / `titles`.

| Word | The confusion (two or more senses in use) | Evidence (record id or Plan key, quote of at most 15 words) | Who wrote it | Count |
| --- | --- | --- | --- | --- |
| PR (`PR`, `PRs`) | `object` (a Workflow-to-be) and `pull request` in one sentence: a "PR" binds to a "GitHub PR"; only a qualifier tells them apart | ctx:90 pro-b43 "merged GitHub PR as its own PR"; titles:315 old-pr "a GitHub PR created after its PR started" | agent (Manager or Lead; unverified which); titles: Lead (unverified) | ctx 191 / titles 306 [PR (any form)] |
| PR as Plan / as the Lead's own | the word 'PR' used where the Plan, its Lead or its batch of Jobs is meant: Leads and Plans own "a PR", Plans are named "(PR key)", work is parked "in a started PR" | ctx:177 pro-d36 "a Lead's PR waits only"; ctx:325 pro-n24 "Plan esc-digest (PR digest)"; ctx:56 pro-b26 "Jobs in a started PR" | agent (Manager or Lead; unverified which) | ctx 15 / titles 0 [Plan x (PR y) pattern] |
| merged / merge | a Job merged into a Lead Branch, an Attempt recorded `merged`, a PR row with status merged, and a pull request merged on GitHub: one word, four events; one defect came from it | ctx:390 pro-n91 "PR marked merged"; ctx:122 pro-p60 "merge recorded merged"; ctx:36 pro-b16 "work merged into the Lead Branch" | agent (Manager or Lead; unverified which); pro-p60 text: Lead of preco-connected-flow (ctx:123) | ctx 133 / titles 208 [merge] |
| open | the open Job, the PR status `open`, `pr open`, the open step and a pull request being open on GitHub | ctx:261 pro-d126 "no pull request opens (open Job refused to start)"; ctx:380 pro-n81 "the PR has no open Job" | agent (Manager or Lead; unverified which) | ctx 93 / titles 23 [open] |
| stacked | `object` sense: the PR's base is another PR of the Plan; `pull request` sense: its base is not the default branch; a Lead's note says they differ | ctx:410 pro-n111 "base is no PR's Lead Branch"; ctx:254 pro-d118 "A stacked pull request"; ctx:211 pro-d73 "A stacked PR waits" | agent (Manager or Lead; unverified which) | ctx 28 / titles 8 [stacked] |
| base | a PR key, a `<plan>/<key>`, a Lead Branch or a remote branch name in `--base`; the same value read as a branch failed with `base_not_found` | ctx:281 pro-d133 "a bare PR key in --base"; ctx:282 pro-d134 "any other value is a branch" | agent (Manager or Lead; unverified which) | ctx 31 / titles 6 [base] |
| workflow | GitHub Actions workflow, a Jira workflow, cpd 1's name for Plan, and the new cpd object (pro-d136) | ctx:420 pro-n121 "EEC workflow names"; ctx:236 pro-d100 "a workflow listed in the repo"; ctx:283 pro-d136 "cpd 1's Workflow (= Plan" | agent (Manager or Lead; unverified which) | ctx 6 / titles 0 [workflow] |
| review | Plan review (the approval step), a review Job (commits nothing), a GitHub review of a pull request, an Inbox review item, a review request from others | ctx:205 pro-d67 "pr-post's Plan review waits"; ctx:73 pro-p35 "holds only a merged review Job"; ctx:202 pro-d64 "A review that requests changes" | agent (Manager or Lead; unverified which) | ctx 65 / titles 29 [review] |
| Gate / check / CI | `Gate` (the repo pre-merge hook), "PR Gate" (a rule that refuses to open), GitHub checks and CI | ctx:127 pro-b61 "a PR Gate for missionlane repos"; ctx:409 pro-n110 "The Gate and CI passed"; ctx:202 pro-d64 "its CI is green" | agent (Manager or Lead; unverified which) | ctx 37 / titles 19 [Gate], ctx 43 / titles 13 [check], ctx 6 / titles 0 [CI] |
| deploy / dispatch | cockpit column `Deploy` (also "Deploy/Dispatch column"), Plan phase `deploy` / `dispatch`, a GitHub Actions deploy run (Inbox kind), the Job dispatch step | ctx:135 pro-b65 "Deploy/Dispatch column"; ctx:236 pro-d100 "a deploy is a GitHub Actions run waiting"; ctx:258 pro-d123 "job dispatch refuse to start" | agent (Manager or Lead; unverified which) | ctx 36 / titles 12 [deploy], ctx 12 / titles 15 [dispatch] |
| draft | a draft Job (a status), a draft pull request on GitHub, and a PR holding draft Jobs | ctx:405 pro-n106 "PR lead-branch (draft)"; ctx:405 pro-n106 "(draft Jobs)"; ctx:55 pro-p26 "no all-draft started PR"; ctx:255 pro-d119 "not draft, merge_state clean" | agent (Manager or Lead; unverified which) | ctx 13 / titles 1 [draft] |
| tier / Tier | Worker Tier (H0-H3, S0-S3), the cockpit's dot-urgency tier, the Inbox's notice tier | ctx:31 pro-p14 "Tree order: dot tier"; ctx:37 pro-p17 "Worker Tiers H0-H4 and S0-S3"; ctx:196 pro-d58 "cpd's notice tier" | agent (Manager or Lead; unverified which) | ctx 40 / titles 24 [tier] |
| owner | the Context's owner (a policy: clayton-duarte, missionlane), the GitHub `owner` of `owner/repo#n`, and "owned by a Plan" (a PR with a Lead) | ctx:5 pro-p1 "Correct a Context's owner"; ctx:246 pro-d110 "owner/repo#n"; ctx:254 pro-d118 "owned by a Plan or not" | agent (Manager or Lead; unverified which) | ctx 64 / titles 20 [owner] |
| ready | a Job state (`Ready Jobs`), "ready to post", "ready to merge", and the Inbox kind `pr-ready` | ctx:191 pro-d52 "Ready Jobs"; ctx:116 pro-p57 "never ready to merge"; ctx:268 pro-d130 "ready to post" | agent (Manager or Lead; unverified which) | ctx 27 / titles 6 [ready] |
| run | an Attempt, a GitHub Actions run, an e2e test run, `cpd resource run` and `cpd session run` | ctx:236 pro-d100 "a GitHub Actions run waiting"; ctx:212 pro-d74 "cpd resource run <name>"; ctx:61 pro-p29 "e2e runs" | agent (Manager or Lead; unverified which) | ctx 73 / titles 28 [run] |
| post | `cpd pr post` (a Slack message), `cpd board post`, the Inbox `Post` action | ctx:216 pro-d78 "'cpd board post' stays for information"; ctx:203 pro-d65 "one post per PR"; ctx:72 pro-b34 "posts a PR for review" | agent (Manager or Lead; unverified which) | ctx 56 / titles 16 [post] |
| adopt | `adopt` is a Plan id, a PR key, the feature "PR adoption" and the flag `--adopt` | ctx:23 pro-p10 "Land PR adoption"; ctx:348 pro-n48 "Plan adopt (PR adopt"; titles:155 pr-adopt "PR adopt (cancelled)" | agent (Manager or Lead; unverified which); titles: cpd text | ctx 29 / titles 4 [adopt] |
| escalation | a Worker's Escalation to its Lead (`cpd ask`) and a Lead's escalation to the Manager (`session send`) | ctx:324 pro-n23 "a Worker's Escalation"; ctx:216 pro-d78 "escalated the stale-Tier bug (pro-n86) by board post" | agent (Manager or Lead; unverified which) | ctx 11 / titles 4 [escalat] |
| ticket | Jira ticket; the Plan key is also a Jira key (titles name Plans `EEC-3217`) | ctx:262 pro-d127 "never defaulted from the Plan's key"; ctx:126 pro-b61 "its own Jira ticket" | agent (Manager or Lead; unverified which) | ctx 13 / titles 4 [ticket] |
| hold | a held Job (`update-hold`), an incident hold (all work), a hold on a Gate | ctx:175 pro-d34 "a PR is never held"; ctx:178 pro-d37 "all work is on hold"; ctx:343 pro-n43 "Incident hold enforced" | agent (Manager or Lead; unverified which) | ctx 32 / titles 4 [hold] |

Words in the Plan brief's list that show NO conflict in these inputs: `ship` (0 / 0), `task` (0 / 0), `epic` (0 / 0). `attempt` is used for the cpd Attempt only in the lines I read (ctx 39 lines; unverified for the tails of the longest records). `phase` appears in ctx on 2 lines and never as the old 'PR' phase.

## The User's own words

Only records whose text says the User decided, asked, reported or spoke. The table gives the date in the record header (a UTC date) and, where the record names one, the User's own time in EDT. Where the User's exact words are quoted in the record I give them in quotes; otherwise the 'What' column is the agent's record of the decision (the file does not hold the User's sentence).

| Word | What the User said it means or asked for | Record | Date |
| --- | --- | --- | --- |
| PR | "PR means only the GitHub pull request" (ctx:283) | pro-d136 | 2026-10-28 (User, 2026-10-28 EDT) |
| Workflow | the cpd object with repo, base, Lead Branch, Lead worktree, Jobs, ending in one pull request, "Plan > Workflows > Jobs"; returns from cpd 1 with a new meaning; a GitHub Actions workflow is written in full | pro-d136 | 2026-10-28 (User, 2026-10-28 EDT) |
| rename scope | history keeps the old words; no alias, shim or deprecation period; old object commands become usage errors | pro-d138 | 2026-10-28 (User, 2026-10-28 EDT) |
| glossary | CONTEXT.md audited in place; an audit lists every term with all senses before contested ones are settled; a guard test fails when a retired term returns | pro-d136 | 2026-10-28 (User, 2026-10-28 EDT) |
| stacking / --base | stacking (`--base <pr-key>`) is the default for a same-repo dependency; `--needs` only across repos | pro-d133 | 2026-10-28 (User, 2026-10-28 09:2x EDT) |
| --base <key> | names a base PR when the key is a PR of the same Plan and repo; any other value is a branch | pro-d134 | 2026-10-28 (User, 2026-10-28 EDT) |
| restack | after the lock, a waiting PR may only stack on a PR it already needs | pro-d137 | 2026-10-28 (User, 2026-10-28 EDT) |
| PR hold | "a PR is never held"; a conflict is a sign of work that was not ordered (ctx:175) | pro-d34 | 2026-10-25 (User, 2026-10-24) |
| PR wait | a Lead's PR waits only on the needs linked to it; never on red main or another Plan | pro-d36 | 2026-10-25 (User, 2026-10-25) |
| Plan dependencies | no cross-Lead or cross-Plan dependencies; only work inside one Plan depends on other work (Jobs, and PRs of the same Plan) | pro-d25 | 2026-10-24 (User, 2026-10-24) |
| rebase | "we only rebase if the PR requires it (conflicts)" (ctx:239); conflicts are always a Job | pro-d103 | 2026-10-27 (User, 2026-10-27 10:3x EDT) |
| PR row / GitHub PR | a PR row binds only to a GitHub PR created at or after the row's `started_at` | pro-d85 | 2026-10-27 (User, Plan old-pr, 2026-10-26) |
| pull request (authored) | every open pull request authored by the User is ONE state-driven Inbox row | pro-d130 | 2026-10-27 (User, 2026-10-27 EDT) |
| stacked pull request | a pull request whose base is not the default branch is never ready to merge | pro-d118 | 2026-10-27 (User, 2026-10-27 14:09 EDT) |
| failed Job "being worked on" | "when a failed job is being worked on the tui was supposed to render a red loading dot" (ctx:226) | pro-d88 | 2026-10-27 (User, 2026-10-26 22:11 EDT) |
| peer review | protocol for missionlane repos: ready to post, ask the User, peers review, changes requested handled at once | pro-d64 | 2026-10-26 (User, 2026-10-26 14:53 EDT) |
| review Job | a PR whose merged Jobs are all review Jobs is ended by `pr cancel` | pro-d77 | 2026-10-27 (User, 2026-10-26 21:25 EDT) |
| review thread | cpd never resolves a review thread; people resolve | pro-d68 | 2026-10-27 (User, 2026-10-26 15:16 EDT) |
| Worker Tiers | H0-H4 or S0-S3 (pro-d53), narrowed to H0-H3 and S0-S3 (pro-d82) | pro-d53, pro-d82 | 2026-10-26, 2026-10-27 |
| Lead | "the Lead is the User's interface to the code"; a Lead runs a command the User explicitly asks for | pro-d56 | 2026-10-26 (User, 2026-10-26 11:53 EDT) |
| resource, slot, tenant | "a resource is anything with a limited number of slots for concurrent use"; a slot is one concurrent unit; a tenant occupies it | pro-d92 | 2026-10-27 (User, 2026-10-27 09:2x EDT) |
| deploy | a deploy is a GitHub Actions run waiting for the User's approval; cpd never approves | pro-d100, pro-d98 | 2026-10-27 (User, 2026-10-27 EDT) |
| Merge (Inbox) | no cockpit action takes more than 2 clicks; Merge needs no confirmation | pro-d111 | 2026-10-27 (User, 2026-10-27 13:20 EDT) |
| Lead Branch | `<plan>/<key>` without the `cpd/` prefix | pro-d112 | 2026-10-27 (User, 2026-10-27 13:25 EDT) |
| cpd | cpd is a CLI; the pi extension is one interface to it | pro-d109 | 2026-10-27 (User, 2026-10-27 13:15 EDT) |
| tier order | reordering by activity was "not a great idea": tier first, then newest first | pro-d48 | 2026-10-26 (date of the User remark not stated) |
| Escalation | a Worker's Escalation must not be reported as its Lead's own ask | pro-n23 | 2026-10-23 (User, 2026-10-23) |
| provider error | "all 500s" count (the User said 'all 500s') | pro-d102 | 2026-10-27 (User, 2026-10-27) |
| time | times to people in the machine's local zone, never UTC | pro-d41 | 2026-10-26 (User, 2026-10-26) |

## Words agents invented

Terms used as terms of art in the records and Plan titles that `CONTEXT.md` does not define. A term is listed when `rg -c -i -F '<term>' CONTEXT.md` is 0 (last column) and it appears in `ctx` (count in the third column, lines). Citation is the first line of `ctx` that holds it. Whether a term deserves a glossary entry is for the User; I only report.

| Term | First use (path:line, record) | Lines in ctx | Lines in CONTEXT.md |
| --- | --- | --- | --- |
| Deploy column | context-cpd.md:39 (pro-p18) | 14 | 0 |
| incident mode | context-cpd.md:178 (pro-d37) | 3 | 0 |
| tripwire | context-cpd.md:177 (pro-d36) | 2 | 0 |
| Verdict | context-cpd.md:108 (pro-p53) | 5 | 2 |
| Claim = cpd inbox claim | context-cpd.md:196 (pro-d58) | 1 | 0 |
| dot-urgency tier | context-cpd.md:32 (  - brief pro-b14) | 2 | 0 |
| Pulse | context-cpd.md:187 (pro-d48) | 3 | 0 |
| janitor | context-cpd.md:208 (pro-d70) | 4 | 0 |
| adopted PR | context-cpd.md:157 (pro-d15) | 6 | 0 |
| ready to merge | context-cpd.md:116 (pro-p57) | 10 | 0 |
| ready to post | context-cpd.md:202 (pro-d64) | 4 | 0 |
| pr-ready | context-cpd.md:100 (pro-p49) | 13 | 0 |
| is steered | context-cpd.md:239 (pro-d103) | 1 | 0 |
| backfill | context-cpd.md:230 (pro-d92) | 2 | 0 |
| earmark | context-cpd.md:242 (pro-d106) | 1 | 0 |
| LEAK-FAIL | context-cpd.md:184 (pro-d43) | 6 | 0 |
| quarantine | context-cpd.md:184 (pro-d43) | 5 | 0 |
| Job pill | context-cpd.md:48 (  - brief pro-b22) | 6 | 0 |
| questionnaire | context-cpd.md:206 (pro-d68) | 3 | 0 |
| Ad-hoc Jobs | context-cpd.md:192 (pro-d53) | 2 | 0 |
| drive-by | context-cpd.md:378 (pro-n79) | 1 | 0 |
| its leaves | context-cpd.md:239 (pro-d103) | 1 | 0 |
| implied need | context-cpd.md:211 (pro-d73) | 2 | 0 |
| hard cut | context-cpd.md:168 (pro-d26) | 2 | 0 |
| reaction queue | context-cpd.md:109 (  - brief pro-b52) | 3 | 0 |
| 'work' Jobs | context-cpd.md:206 (pro-d68) | 1 | 0 |
| Feedback fix | context-cpd.md:104 (pro-p51) | 6 | 1 |
| stopgap | context-cpd.md:331 (pro-n30) | 3 | 0 |
| Interim | context-cpd.md:173 (pro-d31) | 4 | 0 |
| Worker cap | context-cpd.md:186 (pro-d47) | 3 | 0 |
| urgent PR | context-cpd.md:178 (pro-d37) | 2 | 0 |
| red loader | context-cpd.md:93 (pro-p45) | 3 | 0 |
| Inbox row | context-cpd.md:134 (pro-p66) | 3 | 0 |
| cockpit | context-cpd.md:32 (  - brief pro-b14) | 22 | 3 |

Terms that CONTEXT.md does define but that agents use in a second sense are in `Confusions` (for example `Gate`, `Merged`, `Tier`, `Escalation`). Note for the glossary part: `CONTEXT.md` itself defines 'PR' three times, as `**PR** (phase)` (CONTEXT.md:29), `**PR** (step)` (CONTEXT.md:71) and `**PR** (entity, new: R34)` (CONTEXT.md:111); I did not audit them.

## PR in practice

Every way 'PR', 'PRs' and 'pull request' is used in `ctx`, one row per usage pattern. Count = `rg -c` lines in `ctx`. `titles` is cpd text: its 290 `- PR <key>` lines are all `object` by the file's own rule (`rg -c '^- PR ' titles.md`). Patterns overlap (one line can match several); the sense is the one in the two quotes; each quote is at most 15 words.

| Usage pattern | Sense | Count (ctx) | `rg -c` command | Quote 1 | Quote 2 |
| --- | --- | --- | --- | --- | --- |
| cpd command or flag naming the object | object | 26 | `rg -c -e 'pr add\|--pr\b'` | "needs linked to it (pr add --needs)" ctx:177 pro-d36 | "job add --pr <key> to a PR whose open Job has merged" ctx:239 pro-d103 |
| PR key, row, needs, status as a noun | object | 16 | `rg -c -e '\bPR (key\|keys\|row\|rows\|needs\|status)\b'` | "a PR key, not a Job key" ctx:291 pro-d45 | "tied to the research it waits on by PR needs stated at pr add" ctx:291 pro-d45 |
| possessive: a PR's Jobs, Lead Branch, worktree | object | 10 | `rg -c -e 'PR's (Jobs?\|needs\|status\|Lead\|worktree\|key\|row\|base\|Lead Branch)'` | "A PR's Lead Branch is <plan>/<key>" ctx:248 pro-d112 | "A PR's Lead worktree is removed whole or left untouched" ctx:209 pro-d71 |
| PR with a state or relation word (base, stacked, waiting, started, open, cancelled, merged, adopted, authored) | mixed (object for base, stacked, waiting, started, cancelled; pull request for open authored) | 36 | `rg -c -e '(base\|stacked\|waiting\|started\|open\|cancelled\|merged\|adopted\|authored) PRs?\b'` | "A cancelled PR's worktree and branch are removed by the clean-up" ctx:210 pro-d72 | "Move authored PRs from Deploy column into state-colored Inbox rows" ctx:134 pro-p66 |
| 'GitHub PR' (qualifier added to mean the pull request) | pull request | 4 | `rg -c -e 'GitHub PRs?'` | "already merged GitHub PR as its own PR" ctx:90 pro-b43 | "A PR row binds only to a GitHub PR" ctx:223 pro-d85 |
| 'pull request' spelled out | pull request | 17 | `rg -c -e 'pull requests?'` | "only for pull requests authored by the User" ctx:202 pro-d64 | "A stacked pull request (any whose base is not the repo's default branch" ctx:254 pro-d118 |
| pull request by number (#NNN) | pull request (unverified that every match is a pull request number) | 32 | `rg -c -e '#[0-9]{3,4}\b'` | "Land PR adoption (revive #304) for card-dashboard #3748/#3749" ctx:23 pro-p10 | "cpd install has been refused since #526 merged" ctx:409 pro-n110 |
| PR plus a pull-request attribute (description, body, template, link, title, comment, poll, post, merges, ready) | pull request (mixed when it names the PR row) | 21 | `rg -c -e 'PR (description\|body\|template\|link\|links\|title\|comment\|comments\|poll\|post\|merges\|merged\|ready)'` | "The Lead writes the PR description" ctx:232 pro-d94 | "Done when the PR merges" ctx:133 pro-b64 |
| 'PR open', 'the PR opens' (the open step) | step (mixed with the object status open) | 5 | `rg -c -e 'PR (open\|opens\|opened)\b'` | "never at PR open" ctx:133 pro-b64 | "the PR opens on a run with no LEAK" ctx:184 pro-d43 |
| 'PR phase' / 'PR stage' (the old Plan phase) | phase: none in ctx or titles (the phase shows only as phase dispatch and phase deploy) | 0 | `rg -c -e 'PR (phase\|stage)\|(phase\|stage) PR'` | "in phase dispatch" ctx:350 pro-n50 | titles:154 "phase deploy" (Plan pr-adopt: a Plan phase, not a PR) |
| a Plan named with its PR in brackets | Plan (the sense the User reported) | 15 | `rg -c -e 'Plan [a-z0-9-]+ \(PRs? '` | "Plan esc-digest (PR digest)" ctx:325 pro-n24 | "Plan tree-order (PR tree, repo cpd)" ctx:355 pro-n55 |
| a Lead's PR / a Lead hold a PR | Plan (a Lead owns one Plan) or object | 2 | `rg -c -e 'Lead.s PRs?\|Lead hold a PR'` | "a Lead's PR waits only on the needs" ctx:177 pro-d36 | "Let a Lead hold a PR's update Jobs" ctx:18 pro-p7 |
| PR as a batch of Jobs | Job batch | 4 | `rg -c -e 'in (a\|one\|the) (started \|same )?PRs?\b'` | "Leads park its work as all-draft Jobs in a started PR" ctx:56 pro-b26 | "keep research and its follow-up in one PR" ctx:28 pro-b12 |
| the Jobs of a PR (owner of the Jobs) | object | 6 | `rg -c -e 'PR's Jobs?\|Jobs? of (the )?PR'` | "the PR's newest Jobs" ctx:105 pro-b50 | "a PR whose merged Jobs are all review Jobs" ctx:215 pro-d77 |
| PR as a diff (what it changes) or a branch of work | mixed (the branch, its diff or the pull request; unclear) | 5 | `rg -c -e 'PRs? (that\|which) (adds\|touches\|edits\|holds\|conflicts)\|PR (touches\|edits\|adds\|overlaps\|changes)'` | "a PR that adds a migration conflicts on that line" ctx:257 pro-d122 | "Plan update-hold (PR conflict, queued after its review)" ctx:338 pro-n38 |
| PR [key] merged / started / locked / lands (a key used as a name) | mixed (object key, but merged and lands are pull-request events) | 14 | `rg -c -e '\bPR [a-z-]+ (merged\|started\|locked\|opens?\|lands?)'` | "PR core merged and installed" ctx:165 pro-d23 | "owner-fix's PRs land first" ctx:321 pro-n20 |
| 'PR' as a rule on opening ('PR Gate') | other (a Gate rule that stops a pull request opening) | 3 | `rg -c -e 'PR Gate'` | "a PR Gate for missionlane repos" ctx:127 pro-b61 | "it is a PR Gate for missionlane repos" ctx:261 pro-d126 |
| 'PR adoption' (feature name) | pull request (a pull request cpd did not open) | 5 | `rg -c -e 'PR adoption'` | "Land PR adoption" ctx:23 pro-p10 | "PR adoption landed" ctx:353 pro-n53 |
| posting a PR ('post a PR for review') | pull request (a Slack post of it) | 3 | `rg -c -e 'a PR for review'` | "post a PR for review to the team channel" ctx:71 pro-p34 | "A cpd command posts a PR for review" ctx:72 pro-b34 |
| a stacked/base PR as the base of another | object | 9 | `rg -c -e 'when a PR depends on an earlier PR of the same Plan\|base PR'` | "when a PR depends on an earlier PR of the same Plan" ctx:140 pro-b67 | "A stacked PR waits for its base PR to open" ctx:211 pro-d73 |

How to read it: the User's report holds. The 'Plan' rows (`Plan [key] (PR [key])`, "a Lead's PR", "Jobs in a started PR") are the sentences where the word 'PR' stands for something a Plan or its Lead owns; I found no sentence that calls the Plan itself a PR, and none that uses 'PR' for the old Plan phase. CONTEXT.md:111 defines the same words as `object`, so the reading depends on the reader. A proposal, not a decision: the `mixed` rows (merged, open, stacked, base, a PR's diff) stay ambiguous after the rename unless each sentence also says 'Workflow' or 'pull request'; the User decides.

## Counts

All counts are `rg -c` (lines with at least one match), whole-word with `-w` where shown, case-insensitive with `-i`. Run each in `/tmp/cpd-glassary`; ctx = `context-cpd.md`, titles = `titles.md`.

| Word | `rg -c` command | ctx | titles |
| --- | --- | --- | --- |
| PR (any form) | `rg -c -w -e 'PRs?'` | 191 | 306 |
| PR singular | `rg -c -w -e 'PR'` | 174 | 304 |
| PRs plural | `rg -c -w -e 'PRs'` | 37 | 6 |
| pull request | `rg -c -e 'pull requests?'` | 17 | 3 |
| pull_requests (table) | `rg -c -e 'pull_requests'` | 3 | 0 |
| GitHub PR | `rg -c -e 'GitHub PRs?'` | 4 | 2 |
| workflow | `rg -c -iw -e 'workflows?'` | 6 | 0 |
| GitHub Actions | `rg -c -e 'GitHub Actions'` | 5 | 0 |
| review | `rg -c -iw -e 'review\|reviews\|reviewed\|reviewer\|reviewers'` | 65 | 29 |
| merge | `rg -c -iw -e 'merged\|merges\|merge\|merging'` | 133 | 208 |
| open | `rg -c -iw -e 'open\|opens\|opened'` | 93 | 23 |
| deploy | `rg -c -iw -e 'deploy\|deploys'` | 36 | 12 |
| dispatch | `rg -c -iw -e 'dispatch'` | 12 | 15 |
| ship | `rg -c -iw -e 'ship\|shipped'` | 0 | 0 |
| run | `rg -c -iw -e 'run\|runs'` | 73 | 28 |
| attempt | `rg -c -iw -e 'attempts?'` | 39 | 8 |
| task | `rg -c -iw -e 'tasks?'` | 0 | 0 |
| ticket | `rg -c -iw -e 'tickets?'` | 13 | 4 |
| epic | `rg -c -iw -e 'epics?'` | 0 | 0 |
| Gate | `rg -c -w -e 'Gate'` | 37 | 19 |
| CI | `rg -c -w -e 'CI'` | 6 | 0 |
| check | `rg -c -iw -e 'checks?'` | 43 | 13 |
| draft | `rg -c -iw -e 'draft\|drafts'` | 13 | 1 |
| owner | `rg -c -iw -e 'owner\|owners\|owned\|unowned'` | 64 | 20 |
| tier | `rg -c -iw -e 'tier\|tiers'` | 40 | 24 |
| phase (titles: one per Plan header) | `rg -c -iw -e 'phase'` | 2 | 97 |
| stacked | `rg -c -iw -e 'stacked\|stacking\|stack'` | 28 | 8 |
| base | `rg -c -iw -e 'base'` | 31 | 6 |
| post | `rg -c -iw -e 'post\|posted\|posts\|posting'` | 56 | 16 |
| ready | `rg -c -iw -e 'ready'` | 27 | 6 |
| adopt | `rg -c -iw -e 'adopt\|adopted\|adoption'` | 29 | 4 |
| escalat | `rg -c -i -e 'escalat'` | 11 | 4 |
| hold | `rg -c -iw -e 'hold\|held\|holds'` | 32 | 4 |
| Plan | `rg -c -w -e 'Plans?'` | 197 | 104 |
| Job | `rg -c -w -e 'Jobs?'` | 116 | 26 |
| pr add | `rg -c -e 'pr add'` | 14 | 1 |
| --pr flag | `rg -c -e '--pr\b'` | 12 | 0 |
| pr open | `rg -c -e 'pr open'` | 4 | 1 |
| pr post | `rg -c -e 'pr post'` | 19 | 4 |
| pr poll | `rg -c -e 'pr poll'` | 5 | 3 |
| pr-feedback | `rg -c -e 'pr-feedback'` | 7 | 1 |
| pr-ready | `rg -c -e 'pr-ready'` | 13 | 2 |
| PR phase / PR stage | `rg -c -e 'PR (phase\|stage)\|(phase\|stage) PR'` | 0 | 0 |
| Lead Branch | `rg -c -e 'Lead Branch'` | 14 | 2 |
| titles: PR lines (object by file rule) | `rg -c -e '^- PR '` | 0 | 290 |
| titles: Plan header lines | `rg -c -e '^## Plan'` | 1 | 97 |
