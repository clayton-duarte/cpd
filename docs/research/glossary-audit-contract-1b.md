# Glossary audit: docs/contract.md section 3 (state machines, engine, CPD flow)

Notation: `L<n> «text»` is a verbatim quote from `docs/contract.md` line `<n>` (at most 15 words). Sense codes: `object`, `pull request`, `phase`, `step`, `mixed`, `other`. Nothing was changed outside this file. Proposed wordings are proposals: the User decides.

## Coverage

| Path | Lines | Read | Notes |
|---|---|---|---|
| docs/contract.md | 749-751 (`## 3. State machines`, intro) | in full | |
| docs/contract.md | 753-792 (3.1 Plan phases, Grounding, `plan new`, Manager, retired grounded Lead) | in full | |
| docs/contract.md | 793-857 (3.2 Job statuses) | in full | |
| docs/contract.md | 858-861 (3.3 Escalations in the Job's life) | in full | |
| docs/contract.md | 862-967 (3.4 Attempt statuses, driver A1-A9, resources, tenancy, reload) | in full | |
| docs/contract.md | 968-980 (3.4a Display Tier derivation) | in full | |
| docs/contract.md | 981-990 (3.5 Escalation states) | in full | |
| docs/contract.md | 991-1010 (3.6 Session states) | in full | |
| docs/contract.md | 1011-1095 (3.7 engine, E1-E17, install, adapter swap) | in full | |
| docs/contract.md | 1096-1165 (3.8 PR statuses) | in full | |
| docs/contract.md | 1166-1179 (Stacked PRs, unnumbered under 3.8) | in full | |
| docs/contract.md | 1180-1205 (3.9 The CPD flow) | in full | |

- The part is lines 749-1205, 457 lines, 1 file. The Spec says "about lines 724 to 1180"; in the checked-out file `## 3. State machines` is at line 749 and `## 4. Storage` at line 1206, so the range above is the real one.
- Read in full with the read tool in chunks of 100-160 lines. Skimmed: nothing. Not read: every other line of `docs/contract.md` (other Jobs' parts), and `CONTEXT.md` (the glossary: not opened, so "undefined in the glossary" claims below are unverified).
- Several table rows are one very long line (E3, E4, E5, E8, E15, 3.7 install text). I read them whole but spot-quoting uses the line number of the row.
- Commands behind each count (run from the repo root; `P` = `sed -n 749,1205p docs/contract.md`):
  - uppercase `PR`/`PRs`: `P | rg -o '\bPRs?\b' | wc -l` = 460
  - `pull request`: `P | rg -o '(?i)pull requests?' | wc -l` = 65
  - bare lowercase `pr` tokens: `P | rg -o -e '\bpr\b' | wc -l` = 237 (overlaps the families below)
  - families: `P | rg -o '\bpr (add|cancel|start|edit|describe)\b'` = 66; `'\bpr (open|rebase|push)\b'` = 54; `'\bpr (poll|merge|view|post|reply)\b'` = 21; `'--pr\b'` = 14; `'--until-pr|until_pr'` = 9; `'pr-(started|added|edited|cancelled|base-changed|cancel-open)'` = 24; `'pr-(opened|merged)'` = 8; `'pr-(feedback|ready|poll-failing)|pr_posts'` = 14; `'pull_requests?\b'` = 5; `'gate\.pr'` = 5; `'jobs\.pr'` = 2; `'\bpr_(in_use|other_plan|locked|ended|start_failed|exists|not_ready|not_found|empty|adopted)\b'` = 39; `'\bpr_(not_closed|closed|title_required|unopened)\b'` = 5; `'\bpr_(poll_seconds|poll_transient_minutes|open_escalate_minutes)\b'` = 6; `'\bpr_(number|url|opened_at)\b'` = 4
  - per-section split of uppercase `PR`: the same `rg -o '\bPRs?\b' | wc -l` over each range of the table in `## PR occurrences` (sums to 460).
  - `workflow`: `P | rg -n -i 'workflow'` lists lines 854, 1021 (two uses) and 1023 (`workflows`, GitHub Actions): 3 lines, 4 uses.
- The `PR` mapping per token below was made by reading each line in context; the grouped `object` rows are by rule (the token names a cpd PR key, its status, needs, repo, base, Lead Branch or Jobs) and were counted by the exceptions that I listed one by one: 460 minus the individually listed rows.

## Terms

One row per word used in more than one sense, loosely, stale, wrong or colliding. The proposed wording never decides a contested term.

| Term | Where (path:line) | Sense as used here | Collision | Verdict | Proposed wording |
|---|---|---|---|---|---|
| PR (the object) | docs/contract.md:774, 1096, 1166 | L774 «at least one PR that is not `cancelled`»: the cpd object (key, needs, base, Lead Branch, status) | internal: also the pull request (L1127 «the PR's pull request `merged`»), the old Plan phase `pr` (L768), the Deploy step `PR` (L1194) | overloaded | Workflow (decided by pro-d136/pro-d138); `PR` only for the pull request |
| phase `pr` (old name of `deploy`) | docs/contract.md:768 | L768 «the phase `deploy` was called `pr`»: history note about migration 0008 | internal: object `PR`; the new Workflow | stale (history, keep as history) | keep; it names the old word on purpose; ensure it says "pull request" never "PR phase" |
| phase / stage / step | docs/contract.md:755, 1182, 1194, 1198 | L755 «The phases are the cpd side of the CPD flow»; stage = Context/Plan/Deploy; step = Grounding ... Addressing; also engine steps E1-E17, driver steps A1-A9, install steps | internal: `step` also names §7.6 steps, "Stacked PRs" step 7, "step 10's transaction", install steps 1-3; `phase` also Plan phase vs Session state vs Job status | overloaded | keep stage/step/phase as in CONTEXT.md (unverified: not read); name engine items "ticks"/"engine steps" and driver items "driver steps" once |
| Plan / `plan` | docs/contract.md:753, 760, 1189 | entity Plan; phase `plan` and command `plan phase plan` (L760 «grounding --> plan: plan phase plan»); stage Plan (L1189 «\| Plan \| Dividing \| `plan` \|») | internal: three meanings share one word; `plan phase plan` reads as a tautology | overloaded | keep Plan (entity, stage); consider writing the phase as "the `plan` phase" always |
| Deploy / `deploy` | docs/contract.md:1182, 1198, 1023, 1026 | stage Deploy; phase `deploy`; L1198 «The Deploy stage runs from the end of `dispatch`» | internal: stage (starts in `dispatch`) is wider than the phase; upstream: GitHub deployments (E5 «The deploy read», Inbox `deploy`, `[deploys] workflows`) | overloaded | say "the Deploy stage" vs "phase `deploy`" every time; write GitHub's as "GitHub deployment" |
| Dispatch / `dispatch` / Dispatching / `job dispatch` | docs/contract.md:761, 824, 1101, 1192 | phase `dispatch` (L761 «plan --> dispatch: plan lock»); object status `dispatch` (L1101 «engine E7 or pr start»); step Dispatching (L1192 «the engine runs the Jobs»); command `job dispatch` (L824 «`job dispatch` by hand») | internal: four meanings; a Plan can be in phase `dispatch` while its Workflows are `waiting`, `dispatch` or `open` | overloaded | keep the phase and step; propose a distinct Workflow status name as a proposal only; keep `job dispatch` |
| open / opened / Open Job | docs/contract.md:985, 1124, 1132 | object status `open` (L1124 «`dispatch` \| `open` \| the PR's `open` Job»); Job kind `open` (L1132 «A started PR gets its `open` Job»); Escalation state `open` (L985 «\| (none) \| `open` \|»); pull request state `open`; verb "the pull request is opened" | internal: status of object, kind of Job, state of Escalation, Inbox item `open`; upstream: GitHub pull request state OPEN | overloaded | Workflow status says whether its pull request is open: consider `opened` (pull request opened) vs `started`; keep Job kind `open` only if kinds and statuses never share a word (proposal) |
| merged / Merged / merge | docs/contract.md:779, 825, 1127, 1173 | Job status `merged` (L825 «driver A8: the merge succeeded»); object status `merged` (L1127 «the poll sees the PR's pull request `merged`»); Attempt outcome `merged`; A8 "merge" (rebase and fast-forward); GitHub squash merge; git merge of base into branch (L1173 «`git -C <lead worktree> merge --no-edit origin/<base>`») | internal: Job-merged and object-merged mean different things; upstream: git merge, GitHub merge | overloaded | keep Merged for Jobs; say "pull request merged" and Workflow `merged` only if the pull request is meant |
| draft / Draft | docs/contract.md:848, 1175 | Job status `draft` (L848 «A `draft` Job exists in every phase»; `draft` pill; GitHub draft pull request (L1175 «never as a draft, like every PR») | upstream: GitHub draft pull request | overloaded | "draft pull request" in full when GitHub's |
| Ready / ready | docs/contract.md:840, 1177, 1026 | Ready rule for Jobs (L840 «**Ready.** A `queued` Job whose every Dependency is `merged`»); GitHub "ready for review" (L1177 «a ready PR stays ready»); Inbox `pr-ready` | upstream: GitHub ready-for-review; internal: Inbox kind | loose | "ready (not draft) pull request" for GitHub's |
| waiting / blocked | docs/contract.md:842, 1100, 991-993 | object status `waiting` (L1100); display status `waiting` of a Job or PR that is not Ready (L842 «is shown as `waiting`»); Session state `waiting` vs `blocked` (L993 definition is clear); Worker waiting for Lead | internal: stored status `waiting` of the object and a display `waiting` of a Job or PR with another meaning (not Ready, which includes `dispatch` objects that wait on a need) | overloaded | keep Session `waiting`/`blocked` as defined; give the display status a different word ("held") or say "shown as waiting" always (proposal) |
| review / Reviewing / Review Job | docs/contract.md:799, 846, 1195, 1204 | Plan review (User approves `plan lock`, L1191 «the Plan review: the User reads and decides»); review of the drafts (L799 «an approved review of the drafts»); Review Job (`job add --review`, L846); GitHub pull request review (step Reviewing and Monitoring, L1195); "No local review" (L1204) | upstream: GitHub review; internal: five senses | overloaded | keep Plan review, Review Job; "pull request review" in full for GitHub's |
| check / checks | docs/contract.md:906, 1193 | quick check (L906 «run the quick check and submit»); Mechanical checks step (L1193); CI checks; tamper check, base check, `watch --check`; `CHECK` constraint | upstream: GitHub checks/CI | loose | "Mechanical checks" only for the Deploy step; CI checks as "pull request checks" |
| Gate / `gate.pr` / "PR Gate" | docs/contract.md:1133, 1193, 907 | the repo's pre-merge hook; policy keys `gate.merge` and `gate.pr` (L907 «`gate.pr` is `local` (for `pr rebase`)»); L1133 «no PR Gate (`pre-merge`, `gate.pr`)» | internal: "PR Gate" vs Gate; upstream: GitHub CI | loose | name the key by what it gates ("Gate before a push to the pull request") |
| workflow / Workflow | docs/contract.md:854, 1021, 1023 | L854 «Workflow lead-fork's»: cpd 1 name of a Plan (history); L1021 «the workflow worktree's line only» and «share the workflow worktree»: unverified, probably the Lead worktree; L1023 `[deploys] workflows`: GitHub Actions workflows | internal: becomes the new object name; upstream: GitHub Actions workflow | stale (L854, L1021), ok (L1023 is GitHub's, written "workflows" not in full) | L1021: "Lead worktree" (unverified); L854: leave as history or "Plan"; L1023: "GitHub Actions workflows" in full |
| base / base PR / `base_sha` | docs/contract.md:906, 1168, 1170, 1176 | the object's `base` branch; "base PR" = the object stacked on (derived); `base_sha`; GitHub `baseRefName` (L1177) | upstream: git base, GitHub base branch | overloaded | "base Workflow" after the rename (proposal); `base` stays the branch |
| needs / Dependency | docs/contract.md:774, 1190, 1113 | `--needs` of a PR vs Dependency of a Job; L1190 «order: Dependencies and PR needs» | internal: two words for one idea at two levels | loose | keep (the contract's "need" works for both); consider "Workflow needs" |
| Lead Branch / Lead worktree | docs/contract.md:1160 | per object | internal: ok; "Lead" is the Session role, "Lead Branch" belongs to the object, not the Lead | ok | none |
| Locked / lock / `plan lock` / driver lock | docs/contract.md:774, 852, 918 | Locked Job (not draft); `plan lock` (Plan to dispatch); driver lock `flock`; `locks/*.lock`; "the lock" in 3.4 | internal: four meanings; upstream: flock(2), git locks | overloaded | keep Locked (capital); "driver lock" and "plan lock" in full |
| context (column) vs Context | docs/contract.md:852 | L852 «`goal`, `context`, `touches`»: the Spec column `context` | internal: Context entity | overloaded | rename the Spec column or write "the Spec's `context` field" always |
| Spec / Worker Spec / Specifying | docs/contract.md:852, 924, 1188 | Job Spec (columns, L852); rendered Worker Spec (L924 «Render the Worker Spec to `run/<plan>/specs/<attempt>.md`»); step Specifying (L1188) | internal: step vs artefact | loose | "Worker Spec" for the rendered one; keep |
| ticket / Jira | docs/contract.md:840, 1023 | L840 «The same holds for a ticket»: the Jira key `jira` | upstream: Jira ticket; policy key `pr.jira_required` | loose | "Jira key" |
| Epic | docs/contract.md:785 | L785 «works at the Epic level»: the Manager's level, above Plan | upstream: Jira Epic; glossary definition unverified (CONTEXT.md not read) | loose | verify against CONTEXT.md; avoid unless defined |
| cancel / cancelled / closed | docs/contract.md:1126, 1129, 1149 | object `cancelled` (L1126 «`state = closed`»); `job cancel`; `plan cancel`; GitHub closed pull request | upstream: GitHub closed | overloaded | Workflow `cancelled` vs "pull request closed" |
| session / Session | docs/contract.md:993, 1082, 924 | cpd Session (L993 «A Session is created `stopped`»); pi session file; tmux session (L1082 «`<work>:watch`»); the hidden session (L924 «start the Worker in the hidden session») | upstream: tmux, pi | overloaded | "Session" capitalised for cpd's; "tmux session" in full |
| slot | docs/contract.md:787, 1021 | L787 «the chat slot»: a cockpit pane; concurrency slot (`workers`); resource slot | internal: Slot (resource) vs chat slot | overloaded | "chat slot" vs "worker slot" (proposal) |
| park / parked | docs/contract.md:787, 791, 1036 | parked window (L787 «creates its parked window `ctx-<context>`»); parked fork (L791 «a parked fork that every new Plan forked»); E17 parks a queue row (L1036) | internal | loose | "parked window" / "held row" |
| hold / held | docs/contract.md:1015, 1021, 1134 | incident-mode `is_held` (L1015); "every other Ready Job holds" (L1021); L1134 «What still holds `pr cancel`» | internal | loose | one word per meaning |
| status / state / phase | docs/contract.md:753, 793, 981, 991, 1096, 1126 | headings "Plan phases", "Job statuses", "Escalation states", "Session states", "PR statuses"; `pull_requests.state` is GitHub's, `prs.status` is cpd's (L1126) | upstream: GitHub `state` | loose | keep the headings; say "pull request state" for GitHub's |
| E1-E15 / E1-E16 / E1-E17 | docs/contract.md:751, 1013, 1036 | L751 «E1–E15», L1013 «then E1–E16 in order», table ends at E17 (L1036) | internal | wrong | "E1-E17" in both places |
| tick / poll / queue | docs/contract.md:1013, 1023, 1021 | tick, E5 poll, queue walk | upstream: none | ok | none |

Terms that are fine (one summary line): Plan phase names `grounding`, `done`; Job kinds; Escalation states; Attempt statuses; Tier; Escalation; Event; Inbox; Manager; Lead; Worker; Attempt; Ledger; Session states `working`/`idle`/`stopped`; Locked Job; Quick check; Resolved policy; Policy key; Tenant (in 3.4 only) have one consistent sense in this part.

## PR occurrences

Uppercase `PR`/`PRs`, grouped by range; every `mixed`, `step`, `phase` and `pull request` use is also listed one by one below the groups. After the rename, `object` becomes Workflow (`PRs` becomes Workflows); `pull request` stays PR.

| Where (section or path:line range) | Count | Sense | Quote (at most 15 words) | After the rename |
|---|---|---|---|---|
| 3.1 docs/contract.md:753-792 | 43 | object | L774 «at least one PR that is not `cancelled`» | Workflow |
| 3.2 docs/contract.md:793-857 | 41 | object | L816 «into a PR that is `waiting`» | Workflow |
| 3.3 docs/contract.md:858-861 | 0 | n/a | n/a | n/a |
| 3.4 docs/contract.md:862-967 | 17 | object | L905 «whose PR is stacked and whose base PR is `merged`» | Workflow |
| 3.4a, 3.5, 3.6 docs/contract.md:968-1010 | 0 | n/a | n/a | n/a |
| 3.7 docs/contract.md:1011-1095 | 71 | object | L1013 «the PR starts (E7)» | Workflow |
| 3.8 docs/contract.md:1096-1165 | 157 | object | L1113 «every need is a PR of the same Plan» | Workflow |
| Stacked PRs docs/contract.md:1166-1179 | 98 | object | L1176 «a PR whose status is `waiting` or `dispatch`» | Workflow |
| 3.9 docs/contract.md:1180-1205 | 9 | object | L1189 «the work is split into PRs and Jobs» | Workflow |

Individually listed uses (the exceptions to the rule above; their counts are already taken out of the group counts):

| Where | Count | Sense | Quote (at most 15 words) | After the rename |
|---|---|---|---|---|
| docs/contract.md:762 | 1 | mixed: "opens" is the pull request opened while the object becomes `open` | L762 «the last PR not yet open opens» | Workflow ... opens its pull request |
| docs/contract.md:763 | 1 | mixed: the poll sees the pull request merged; the object becomes `merged` | L763 «the poll sees the last PR merged» | Workflow ... pull request merged |
| docs/contract.md:779 | 1 | mixed: a merged object and a merged pull request carrying a bug | L779 «a merged PR can hold a bug or a regression» | Workflow (pull request in the sentence) |
| docs/contract.md:840 | 1 | mixed: an adopted object keeps the pull request's own body | L840 «an adopted PR keeps its own pull request body» | adopted Workflow |
| docs/contract.md:820 | 1 | pull request: the poll of pull requests | L820 «the PR poll, engine E5» | PR poll |
| docs/contract.md:1022 | 1 | mixed: the Job opens the pull request; the object turns `open` | L1022 «the Job opens the PR» | the Job opens the Workflow's pull request |
| docs/contract.md:1023 | 2 | pull request: the E5 title and the lookup | L1023 «**PR poll**» and L1023 «a failed PR lookup» | PR poll, PR lookup |
| docs/contract.md:1033 | 1 | mixed: a later piece of work of a Plan, that will be a pull request | L1033 «Plan `bifrost`'s later PR» | Workflow or pull request (the User decides) |
| docs/contract.md:1067 | 2 | pull request: install message, "before the PR merges" | L1067 «before the PR merges» | PR |
| docs/contract.md:1122 | 1 | mixed: adopting a pull request makes an object | L1122 «**An adopted PR** (A6)» | adopted Workflow |
| docs/contract.md:1133 | 1 | mixed: the Gate of `gate.pr`, run for the pull request | L1133 «no PR Gate (`pre-merge`, `gate.pr`)» | Gate for the pull request |
| docs/contract.md:1136 | 1 | mixed: opening the object opens its pull request | L1136 «**open the PR later**» | open the Workflow's pull request later |
| docs/contract.md:1139 | 1 | mixed: the object "must be opened" | L1139 «A PR whose Jobs are all `merged` or `cancelled` must be opened» | Workflow |
| docs/contract.md:1164 | 1 | mixed: title says PR closed; the pull request is closed, the object stays `open` | L1164 «**A PR closed without a merge**» | A pull request closed without a merge |
| docs/contract.md:1173 | 1 | mixed: `pr rebase` acts on a Workflow's Lead Branch and its pull request | L1173 «`pr rebase` of a stacked PR is refused `stacked_rebase`» | stacked Workflow |
| docs/contract.md:1175 | 1 | mixed: "draft" is GitHub's; the object is "opened" | L1175 «like every PR» | pull request |
| docs/contract.md:1177 | 1 | mixed: "ready" is GitHub's | L1177 «a ready PR stays ready» | pull request |
| docs/contract.md:1194 | 1 | step: the Deploy step `PR` (the pull request is opened) | L1194 «\| Deploy \| PR \|» | PR (pull request) |
| docs/contract.md:1200 | 1 | step: the step list `PR` | L1200 «Mechanical checks, PR, Addressing» | PR (pull request) |
| docs/contract.md:1200 | 2 | mixed: Lead no longer opens or resolves conflicts | L1200 «opening PRs (`pr open` is the repair» and L1200 «resolving a PR's conflicts by hand» | Workflows / pull requests |
| docs/contract.md:1204 | 1 | mixed: reviewed by people (pull request) after it opens (object) | L1204 «A PR is reviewed after it opens by people» | pull request |
| docs/contract.md:1096 (heading) | 1 | object (already in the 157 of the group; listed as heading) | L1096 «### 3.8 PR statuses (R34)» | Workflow statuses |
| docs/contract.md:1166 (heading, `Stacked PRs`) | 1 | object (already in the 98 of the group; listed as heading) | L1166 «### Stacked PRs (hs-d522, hs-d523)» | Stacked Workflows |

Phrase `pull request` (case-insensitive, 65 uses, all `pull request`: 3.1 2, 3.2 1, 3.4 1, 3.7 10, 3.8 34, Stacked 14, 3.9 3). Examples: L1127 «the PR's pull request `merged`»: pull request; L1175 «so GitHub merges the stacked pull request»: pull request.

Lowercase `pr` tokens (commands, flags, Events, codes, columns), by family; the families overlap the 237 bare `pr` and the `pr_*` codes are outside it:

| Where (family) | Count | Sense | Quote (at most 15 words) | After the rename |
|---|---|---|---|---|
| `pr add`, `pr cancel`, `pr start`, `pr edit`, `pr describe` | 66 | object | L779 «(`pr add`, `pr edit`, `pr cancel`)» | `workflow add` etc. |
| `pr open`, `pr rebase`, `pr push` | 54 | mixed: acts on an object and its pull request | L775 «or `pr open --pr <key>` (§7.6, the repair)» | the User decides: `workflow open`? or keep as pull request verbs |
| `pr poll`, `pr merge`, `pr view` | 21 | pull request | L1023 «run `cpd pr poll` (no `--plan`)» | unchanged (pull request) |
| `--pr <key>` | 14 | object | L816 «`job add --pr <key>` in phase `plan`» | `--workflow <key>` |
| `jobs.pr` | 2 | object | L1058 «the same PR (`jobs.pr`)» | `jobs.workflow` |
| `pr-started`, `pr-added`, `pr-edited`, `pr-cancelled`, `pr-base-changed` | 24 | object | L774 «`pr-started` for each PR started» | `workflow-*` |
| `pr-opened`, `pr-merged` | 8 | mixed: the object moves to `open`/`merged` because the pull request did | L775 «`pr-opened`, then `plan-phase`» | decision needed |
| `pr-feedback`, `pr-ready`, `pr-poll-failing`, `pr_posts` | 14 | pull request | L1026 «`pr-ready` from the `pull_requests` rows» | unchanged |
| `pull_requests` table / `pull_request` | 5 | pull request | L1126 «the PR's `pull_requests` row is `state = closed`» | unchanged |
| `--until-pr`, `until_pr`, `until_pr_not_incident` | 9 | mixed: names an object key, ends when its pull request merges | L1015 «[--until-pr <plan>/<key>]» | decision needed |
| `gate.pr` | 5 | mixed: the Gate for pull-request-affecting commands | L907 «`gate.pr` is `local` (for `pr rebase`)» | decision needed |
| `pr.jira_required` | 1 | mixed: policy key `pr`, named for the pull request | L840 «policy sets `pr.jira_required`» | decision needed |
| policy text `pr` | 2 | mixed: the owner policy named `pr` | L1137 «a User's `pr` policy text» ; L1200 «The owner's `pr` policy text» | decision needed |
| codes `pr_in_use`, `pr_other_plan`, `pr_locked`, `pr_ended`, `pr_start_failed`, `pr_exists`, `pr_not_ready`, `pr_not_found`, `pr_empty`, `pr_adopted` | 39 | object | L1119 «`pr_in_use`» is not quoted here: see L1134 «What still holds `pr cancel` (`pr_in_use`, exit 3» | `workflow_*` |
| codes `pr_not_closed`, `pr_closed`, `pr_title_required`, `pr_unopened` | 5 | mixed: refusals about the pull request's state or title | L1126 «`pr_not_closed`» | decision needed |
| config `pr_poll_seconds`, `pr_poll_transient_minutes` | 4 | pull request | L1023 «`pr_poll_seconds` after the **end** of the previous poll» | unchanged |
| config `pr_open_escalate_minutes` (retired) | 2 | mixed: escalation of an unopened object | L1022 «`pr_open_escalate_minutes`» | history |
| columns `pr_number`, `pr_url`, `pr_opened_at` | 4 | pull request | L1152 «`pr_number`, `pr_url`, `pr_opened_at`» | unchanged |
| `prs` table, `prs.all_merged_at` | 1 | object | L1022 «`prs.all_merged_at` is no longer written» | `workflows` |
| `pr` (old phase name) | 2 | phase | L768 «the phase `deploy` was called `pr`» | history: keep |
| `pr` (queue row column) | 1 | object | L1036 «read the queue row's `plan` and `pr`» | `workflow` |
| bare `pr` residue (placeholders `<pr>`, `{pr}`, paths `core/src/pr/`, prose) | not separated | other: unverified per token | L1060 «Jobs of PR <plan>/<pr> failed» | per token |

## Fix list

| path:line | what to change | why |
|---|---|---|
| docs/contract.md:751 | L751 «E1–E15»: write E1-E17 | the table in 3.7 ends at E17 (L1036) |
| docs/contract.md:1013 | L1013 «then E1–E16 in order»: write E1-E17 | same |
| docs/contract.md:1034 | move the config row `watch.janitor_minutes` out of the engine step table | it is a config key row in the middle of the E-steps table (L1034 `\| `watch.janitor_minutes` \| int \|`), outside this audit's wording scope; reported only because it is a wrong statement of structure |
| docs/contract.md:1096 | heading `### 3.8 PR statuses (R34)` and its states: rename the object to Workflow; decide `open`/`merged` | the statuses mix the object's life with its pull request's: `open` and `merged` are pull request outcomes (L1127) |
| docs/contract.md:1194 | step `PR` in the CPD flow table | step names the pull request being opened; with PR meaning only pull request the word is right, but the table's phase column calls it `dispatch` → `deploy`: say it in prose |
| docs/contract.md:1200 | step list «Mechanical checks, PR, Addressing» | same step |
| docs/contract.md:768 | keep the history sentence about phase `pr` | history keeps old words |
| docs/contract.md:1198 | L1198 «The Deploy stage runs from the end of `dispatch`» | stage and phase `deploy` differ in extent; say so once |
| docs/contract.md:1193 | L1193 «end of `dispatch`» as phase column for Mechanical checks | not a phase name; the column is "cpd phase" |
| docs/contract.md:762, 763, 779, 840, 1022, 1033, 1122, 1133, 1136, 1139, 1164, 1173, 1175, 1177, 1200, 1204 | each `mixed` use of `PR` listed above: choose Workflow or pull request per sentence | the sentence says an object does something only its pull request can do (open, merge, review, draft, ready) |
| docs/contract.md:1015, 907, 840, 1137, 1200 | `--until-pr`, `gate.pr`, `pr.jira_required`, the policy `pr` | mixed identifiers: a Workflow key or flag that is tied to its pull request |
| docs/contract.md:775 | `pr open`, `pr rebase`, `pr push` (54 uses) | mixed commands: they act on a Workflow and its pull request; the User decides the noun |
| docs/contract.md:1021 | L1021 «the workflow worktree's line only» and «share the workflow worktree» | cpd 1 word, collides with the new Workflow; unverified, probably "Lead worktree" |
| docs/contract.md:854 | L854 «Workflow lead-fork's» | cpd 1 name of a Plan (history); a reader will now read it as the new object |
| docs/contract.md:842 | L842 «is shown as `waiting`» | the display `waiting` is also the stored status `waiting` of the object |
| docs/contract.md:1175 | L1175 «like every PR» next to «never as a draft» | "draft" is GitHub's pull request draft; write "draft pull request" |
| docs/contract.md:852 | L852 «`goal`, `context`, `touches`» | the Spec column `context` collides with Context |
| docs/contract.md:840 | L840 «The same holds for a ticket» | ticket is Jira's word for the `jira` key; write "Jira key" |
| docs/contract.md:785 | L785 «works at the Epic level» | Jira collision; unverified against CONTEXT.md |
| docs/contract.md:1166 | unnumbered heading `### Stacked PRs` between 3.8 and 3.9 | cited as "§3.8 Stacked PRs" elsewhere; rule: `Stacked PRs` becomes `Stacked Workflows` |
| rule: every `object` `PR`/`PRs` in 3.1 (43), 3.2 (41), 3.4 (17), 3.7 (71), 3.8 (157), Stacked (98), 3.9 (9) | rewrite to Workflow/Workflows | 436 mechanical rewrites, no row each |
| rule: lowercase `pr` identifiers of the object | `pr add`, `pr cancel`, `pr start`, `pr edit`, `--pr`, `jobs.pr`, `pr-started`/`pr-added`/`pr-edited`/`pr-cancelled`/`pr-base-changed`, `pr_*` codes, `prs` | 66 + 14 + 2 + 24 + 39 + 1 identifiers: rename (no aliases) |
| rule: pull request identifiers | `pr poll`, `pr merge`, `pr view`, `pr-feedback`, `pr-ready`, `pull_requests`, `pr_number`, `pr_url`, `pr_opened_at`, `pr_poll_*` | keep: they name the GitHub pull request |

## Counts

- Files: 1 (docs/contract.md). Lines read in full: 457 (749-1205). Skimmed: 0. Not read: the rest of the file and CONTEXT.md.
- Rows: `## Terms` 31 rows plus 1 summary line; `## PR occurrences` 9 group rows + 23 individually listed rows (2 of them are headings already counted in a group) + 22 lowercase family rows; `## Fix list` 23 rows.
- Uppercase `PR`/`PRs`: 460 = object 436 + pull request 5 + step 2 + mixed 17 + phase 0 + other 0.
- Phrase `pull request`: 65, all `pull request`.
- Lowercase `pr` identifiers, by family count (families partly overlap bare `pr`): object 66 + 14 + 2 + 24 + 39 + 1 + 1 = 147; mixed 54 + 8 + 9 + 5 + 1 + 2 + 5 + 2 = 86; pull request 21 + 14 + 5 + 4 + 4 = 48; phase 2.
- Individually listed mixed `PR` uses: 17 (L762, 763, 779, 840, 1022, 1033, 1122, 1133, 1136, 1139, 1164, 1173, 1175, 1177, 1200 x2, 1204); `step` uses: 2 (L1194, L1200); `phase` uses: 2 lowercase (L768).
- The word `workflow` / `Workflow`: 3 lines (L854, L1021, L1023); L854 and L1021 stale, L1023 GitHub Actions.
