# Glossary audit: docs/commands.md §21 `inbox` to the end (inbox, policy, secret, incident, gate, prune, tag and version, test, mcp, janitor, resource, review)

Part audited by Attempt `aud-commands-4b-a1`. Report only: no other file was changed. Line numbers are those of `docs/commands.md` at the base commit of this worktree; `commands.md:N` means `docs/commands.md` line N. Quotes are verbatim from that line.

## Coverage

The Spec says the part starts "about line 4524"; the heading `## 21. `inbox`` is at line 4513, so the range read starts there. The file has 5632 lines, so the part is `docs/commands.md:4513-5632` (1120 lines), one file. Besides the part I read `CONTEXT.md` lines 26, 69, 75, 79, 107-109, 124-136, 235-236 and 268 (only to check the Gate and Domain entries); those lines are not audited.

| Range | Heading | Read |
|---|---|---|
| 4513-4767 | `## 21. inbox` (21.1 to 21.10) | in full |
| 4768-4862 | `## 22. policy` (22.1) | in full |
| 4863-5171 | `## 23. secret` (23.0 to 23.7) | in full; no PR token in it |
| 5172-5273 | `## 23. incident` (23.1 to 23.3) | in full |
| 5274-5290 | `## 23. gate` (23.1 `gate finish`) | in full |
| 5291-5329 | `## 23. prune` | in full |
| 5330-5369 | `## 23. tag` and `version` (23.1, 23.2) | in full |
| 5370-5402 | `## 23. test` (23.1 to 23.4) | in full |
| 5403-5442 | `## 24. mcp` (24.1) | in full |
| 5443-5526 | `## 29. janitor` | in full |
| 5527-5583 | `## 30. resource` (30.1, 30.2) | in full |
| 5584-5632 | `## 31. review` (31.1) | in full |

Read in full: all 1120 lines, in three reads of 420, 400 and 310 lines (not the 150-line chunks the Spec suggests; each read was complete, nothing was cut). Skimmed: none. Not read: nothing in the part. Not audited, by design: every other file. The `contract §` references inside the part are quoted, not checked. What the part says about code (tables, modules, Events) was not checked against the code: unverified.

The `rg` commands behind the counts (run from `docs/`; `rg` reports lines, the token counts below are a manual tally of the tokens on those lines):

- Lines with a PR-like token in the part: `rg -n -i '\bPRs?\b|pull request|pull_request|pr-|\bpr ' commands.md | awk -F: '$1>=4513'` gives 61 lines (it also hits `pr` inside `cpd version sync --print`, lines 5359-5365; those are false hits and not counted below).
- Token locator: `rg -n -o -i '.{0,40}(\bPRs?\b|pull request|pull_request|pr-[a-z]+|\bpr [a-z]+|--pr|--until-pr|until_pr|pr_[a-z]+|\bpr\b).{0,40}' commands.md | awk -F: '$1>=4513'` gives 97 matches on lines 4515-5630; 3 of them are the false `--print` hits.
- Names the locator misses: `rg -n -o '.{0,30}(_prs?\b|\bprs\b|pr/).{0,20}' commands.md | awk -F: '$1>=4513'` (adds `extract_prs` 4628, the test name ending `_key_pr` 5329, `core/src/pr/poll.rs` 5466).
- Uppercase `PR`/`PRs` in the part: `sed -n '4513,5632p' commands.md | rg -o -w 'PRs?' | wc -l` gives 45 tokens; `sed -n '4513,5632p' commands.md | rg -c -w 'PR|PRs'` gives 26 lines.
- `pull request` (any case): `sed -n '4513,5632p' commands.md | rg -o -i 'pull request' | wc -l` gives 12 tokens.
- `Gate`: `sed -n '4513,5632p' commands.md | rg -o -w 'Gate' | wc -l` gives 3 (two on line 5276, one on line 5580).
- `workflow`: `rg -n -i 'workflow' commands.md | awk -F: '$1>=4513'` gives 1 line (4830).
- `pr-started`: `rg -n 'pr-started' commands.md` gives 4 lines (1083, 2146, 3367, 3692), none in the part.
- Lines with a PR-like token per section (the locator filtered by the section ranges above): inbox 12, policy 11, secret 0, incident 14, gate 0, prune 4, tag/version 3 (all false hits), test 0, mcp 0, janitor 20, resource 1, review 7.

The counts in `## PR occurrences` are my tally of every token (identifier, flag, JSON key, prose "PR"/"pull request") on those lines, so they are larger than the line counts; `## Counts` gives the method and the totals.

## Terms

Collision column: "internal" is another cpd meaning; "upstream" is GitHub, Jira, git, tmux, pi or Slack. Verdicts: ok, loose, overloaded, stale, wrong.

| Term | Where (path:line) | Sense as used here | Collision | Verdict | Proposed wording |
|---|---|---|---|---|---|
| `pr-ready`, `pr-failing`, `pr-changes` (Inbox kinds) | commands.md:4515 | Inbox kinds opened by the PR poll about an open pull request | internal: the `pr-` prefix reads as the object; they follow the pull request, like `pr-feedback` | ok | Keep: names about the pull request stay. `pr-changes` is no longer opened (hs-d470) |
| PR poll | commands.md:4515 | the poll of pull requests | internal: CONTEXT.md entry "PR poll" | ok | Keep; it polls the pull request, not the Workflow |
| `deploy` (Inbox kind) | commands.md:4515 | "one item per deploy run that waits for the User's approval", from `run/deploy-runs.json` | internal: Deploy stage; upstream: GitHub deployment run | overloaded | Say "GitHub deployment run" in full; never "a Deploy" |
| deploy run, run | commands.md:4515 | a GitHub Actions or deployment run | upstream: GitHub Actions run; internal: `resource run`, `test run`, `gate-run` | loose | "GitHub Actions run" in full |
| `review-requested` | commands.md:4595 | Inbox kind for a team review request on a pull request | internal: Review Job, Reviewing; upstream: GitHub review request | overloaded | Keep the kind; write "review request on a pull request" |
| `--pr <key>` against `--pr <url \| owner/repo#n>` | commands.md:4775, 5591 | object key in `policy show`; pull request URL in `review submit` | internal: one flag name, two meanings | overloaded | `policy show` takes `--workflow <key>`; `review submit --pr` keeps the pull request. The User decides the flag of the URL form |
| policy Domain `pr` | commands.md:4796, 4810 | the Domain holds `post`, `feedback.checks` and a gate key | internal: CONTEXT.md:268 lists `pr` as a Domain with `pr.gate`; one name for object and pull request | overloaded | Propose splitting the keys by what each governs; the User decides |
| Domain `gate`, key `pr` in the example | commands.md:4808 | the JSON example has a `gate` Domain with keys `merge` and `pr`; CONTEXT.md:268 lists no `gate` Domain and puts the key at `pr.gate` | internal: Domain list of CONTEXT.md | stale | Check the example against contract §8.3 (which is right: unverified) and redo it with the final key names |
| `until-pr`, `until_pr`, `until_pr_not_incident` | commands.md:5179 | an incident ends when a keyed object merges | internal: Merged | overloaded | `--until-workflow <plan>/<key>`, `until_workflow`; the merge it waits for is the pull request's |
| incident, "hold on scheduling" | commands.md:5174 | "Plan-scoped hold on scheduling" | internal: Draft "holds" the open Job; incident mode "holds" E17 (4725) | loose | "hold" has three uses; no CONTEXT.md entry found by `rg '\*\*Incident' CONTEXT.md` (that pattern only) |
| Gate | commands.md:5276 | "The Gate's leak check": `gate finish` after `scripts/gate-run.sh` | internal: Gate in CONTEXT.md:235, which runs on a PR's Lead Branch | ok | Keep. `gate finish` names no PR (see "The Gate" below) |
| the Gate in hooks | commands.md:5580 | "The Gate and the hooks take no resource by themselves" | internal: Gate, Resource | ok | Keep |
| merged | commands.md:5313 | in `prune`: a merged pull request (from `gh pr list --state merged`) | internal: Merged status of the object (5304, 5449) | overloaded | "merged pull request" at 5313 and 5316; "merged Workflow" at 5304 and 5449 |
| pending PR | commands.md:5463 | a PR whose paths still need cleaning | internal: `pending` outcome (5502) and `pending` queue state (4723); upstream: pending checks | loose | "Workflow to clean"; keep `pending` for the outcome only |
| `core/src/pr/poll.rs`, `merged_prs_to_clean` | commands.md:5465, 5466 | code names of the poll that also cleans the object's worktrees | internal: PR poll against the object | loose | Code names: the identifier rename needs a decision on `core/src/pr/` |
| archived | commands.md:5461 | "every PR of every Plan (archived included)" | internal: archived Session; upstream: 1Password `--archive` (5014) | ok | Keep |
| claim, Claim | commands.md:4678, 4721 | `inbox claim` hides an item and queues an :eyes: reaction | internal: `claimed_at`; no CONTEXT.md entry found by `rg '\*\*Claim' CONTEXT.md` (that pattern only) | loose | Not part of the rename; define it in CONTEXT.md or avoid the capital |
| approve, approval | commands.md:4515, 4890, 5143, 5598 | Deploy approval; `secret approve`; GitHub review `APPROVE` | internal: Approving (Plan stage); upstream: GitHub APPROVE | overloaded | Keep the Plan word for Approving only; say "secret approval" and "review APPROVE" |
| E8, E11, E16, E17 (engine steps) | commands.md:4515, 4721, 4734, 5449 | "engine step E17" | internal: step (Grounding step, Deploy step) | loose | Say "engine step" in full |
| engine E16 for the reaction queue | commands.md:4684, 4690, 4717, 4734 | the runner of `slack-react.ts` | internal: E16 is the janitor (5449) | wrong | The reaction queue is E17 (4719, 4721); these lines say E16 |
| `post`, `pr post`, primary post | commands.md:4628, 4734 | a Slack post; the command that posts a Slack message about the pull request | upstream: Slack post; internal: PR post (CONTEXT.md entry) | loose | "Slack post" for the message, "PR post" for the command |
| queue, queued | commands.md:4721 | the Slack reaction queue | internal: queued update Job | loose | "reaction queue" |
| parked | commands.md:4728 | a refused reaction row is "parked" | internal: park/parked (a Draft, a Job) | loose | "refused row" |
| `workflows` (key of `deploys`) | commands.md:4830 | a list of GitHub Actions workflows | internal: the new Workflow; upstream: GitHub Actions workflow | overloaded | Write "GitHub Actions workflow" in full in prose |
| owner, `Owner(<login>)` | commands.md:4783 | policy Scope `Owner`, `Repo` | upstream: GitHub owner | ok | Keep |
| section numbers | commands.md:4863, 5172, 5274, 5291, 5330, 5370 | six headings numbered `## 23.`; the next are 24, 29, 30, 31 | internal | wrong | Renumber; the references `§24.0`, `§24.3`, `§27` are then wrong too (Fix list) |
| the rest | | Plan, Context, Session, Event, Inbox, Inbox item, Lead, Worker, Manager, Attempt, Job, Lead Branch, Policy, Domain, Scope, Resource, Slot, Tenant, MCP server, Attach: used consistently in the part | | ok | none |

## PR occurrences

Sense codes as in the Spec. Count = tokens (a flag, an identifier, a JSON key or a prose "PR"/"pull request") on the lines of the group. "After the rename": Workflow for `object`, unchanged for `pull request`.

### Listed individually: mixed, other, phase, step

| Where | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| commands.md:4796 | 1 | mixed: Domain `pr` holds keys about the pull request (`post`) and the object (gate); cannot tell which this key is | "`policy/host.toml` sets no `pr.post` table" | open: depends on how the Domain is split |
| commands.md:4808 | 1 | mixed: key `pr` under Domain `gate`; the object or the pull request, and it differs from CONTEXT.md:268 | `"pr": {"value": "local", "source": "owner:` | open: key name set with the Domain |
| commands.md:4810 | 1 | mixed: Domain `pr` itself | `"pr": {"source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml"` | open |
| commands.md:5186 | 1 | mixed: "incident ends when this PR merges": named by the object's key, but the merge is the pull request's | "incident ends when this PR merges" | "ends when this Workflow's pull request merges" |
| commands.md:5466 | 1 | mixed: `core/src/pr/poll.rs`, the poll of pull requests, whose `clean_up` removes the object's worktrees | "`core/src/pr/poll.rs` `clean_up`" | decision on the module name |
| commands.md:5600 | 1 | mixed: `cpd_core::pr::reactions::queue` is a module of the `pr` area that queues a Slack reaction | "`cpd_core::pr::reactions::queue(tx, channel, ts, emoji)`" | decision on the module name |
| commands.md:4721 | 1 | other: `pr-post` is the id of a past Plan (history) | "Plan pr-post, PR `react`" | keep: a Plan id is history |
| commands.md:5323 | 1 | other: `gh pr list`, a subcommand of the GitHub CLI | "`gh pr list --state merged --limit 500`" | keep |
| phase and step | 0 | none in the part: no "PR phase" and no "PR step" found | | none |

### Grouped by section

| Where (section or path:line range) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| 21 `inbox` intro, commands.md:4515 | 6 | pull request: `pr-ready`, `pr-failing`, `pr-changes`, "PR poll's" twice, "pull request's page" | "the PR poll's (`pr-ready` and `pr-failing`" | unchanged |
| 21.4, 21.5, 21.7, commands.md:4595, 4610, 4676, 4678 | 5 | pull request: dismissal and `review-requested` are per pull request | "the dismissal holds for the pull request forever" | unchanged |
| 21.6, commands.md:4628, 4642 | 5 | pull request: PRs extracted from Slack links (`extract_prs`, "the PRs are every", "primary PR", "newest posting of a PR", "one entry per PR") | "the **primary** PR is the first of them" | unchanged |
| 21.9, commands.md:4721, 4723, 4728 | 6 | object: "PR `react`", "PR `claim`", column `pr TEXT`, "both plan and pr", `{plan, pr, file}`, "for the PR" | "`pr TEXT` (NULL for unlinked" | Workflow (a column rename is a migration; applied migrations keep their words) |
| 21.10 and 21.9, commands.md:4734, 4766 | 2 | pull request: `pr post`, `pr_posts` | "`pr post` (§9.8b) runs it" | unchanged |
| 22.1, commands.md:4775-4794, 4833 | 13 | object: flag `--pr` (6), "a PR's repo policy", "Job's PR", "that PR of the selected Plan", `<PR name>`, "a PR whose `name` is null", "Plan, PR or Context exists", `pr_not_found`, "the PR's Plan's" | "`--pr <key>` | that PR of the selected Plan" | Workflow: `--workflow`, `workflow_not_found`, `<Workflow name>` |
| 23 `secret` | 0 | none | | |
| 23 `incident`, commands.md:5179-5222, 5263-5269 | 24 | object: `--until-pr` (6), `until_pr` (13 with the JSON), `until_pr_not_incident`, `pr_not_found`, "PR key", "a PR of one of the named Plans", sample key `plan-a/pr-1` (2) | "`--until-pr <plan>/<key>` | optional; PR key in format" | Workflow: `--until-workflow`, `until_workflow`, `workflow_not_found`, `plan-a/workflow-1` |
| 23 `incident`, commands.md:5219, 5269 | 2 | pull request: "fixing PR ci" (CI of a pull request) | `"reason": "fixing PR ci"` | unchanged |
| 23 `gate` | 0 | the section names no PR | | |
| 23 `prune`, commands.md:5304, 5329 | 3 | object: "each PR whose status is not merged", "a Job of such a PR", test name ending `_key_pr` | "the `lead_branch` of each PR whose status is not merged" | Workflow (test name too) |
| 23 `prune`, commands.md:5313, 5316 | 2 | pull request: "a merged PR P has a head branch", "head of a merged PR that reached main" (head branch, commit, base, from `gh`) | "a merged PR P has a head branch equal to L" | unchanged |
| 23 `tag` and `version`, `test`, `mcp` | 0 | none (3 false hits on `--print`) | | |
| 29 `janitor`, commands.md:5449-5523 | 28 | object: "merged and cancelled PRs", "every PR of every Plan", "pending PR" (2), "the PR's Jobs" (4), `merged_prs_to_clean`, "PR keys", "pending PR path", `"pr": "api"` (3), "PR's status", "the PR is not idle yet", "that PR", "a cancelled PR", "a merged PR", "<status> PR <pr>" in three messages (6), `{plan, pr, ...}`, "next PR", a test name with `_pr_worktrees_` | "Removes the worktrees and branches of merged and cancelled PRs" | Workflow; the Event texts at 5510-5512 change word; the JSON key `pr` becomes `workflow` |
| 29 `janitor`, commands.md:5473 | 2 | pull request: "the pull request's recorded head commit", "a squash-merged PR whose branch GitHub deleted" | "`HEAD` is the pull request's recorded head commit" | unchanged |
| 30 `resource`, commands.md:5529 | 1 | object: "keyed as `pr add` does" | "its `origin` keyed as `pr add` does" | `workflow add` |
| 31 `review`, commands.md:5591-5630 | 8 | pull request: `--pr <url \| owner/repo#n>`, `pr merge` (2), "pull request URL", "check the pull request", "pull request can be read", "on the pull request", "no state of the pull request" | "It takes a pull request URL or PullRef form" | unchanged (the flag name is a decision, see Terms) |

### Inbox kinds, policy keys, commands and messages that name a PR

| Kind | Where | Name | Sense | Note |
|---|---|---|---|---|
| Inbox kind | commands.md:4515 | `pr-ready`, `pr-failing` | pull request | opened by the PR poll |
| Inbox kind | commands.md:4515 | `pr-changes` | pull request | no longer opened (hs-d470) |
| Inbox kind | commands.md:4515, 4595 | `review-requested` | pull request | dismissal and the team review rule are per pull request |
| Inbox kinds | commands.md:4515 | `dialog`, `plan-review`, `loop-guard`, `unread`, `engine-down`, `deploy`, `job-stuck` | no PR | `deploy` is a run: see Terms |
| policy key | commands.md:4796 | `pr.post` | mixed | see above |
| policy Domain | commands.md:4810 | `pr` | mixed | see above |
| policy key | commands.md:4808 | `pr` under `gate` in the example | mixed | differs from CONTEXT.md:268 |
| command | commands.md:4775 | `policy show --pr <key>` | object | |
| command | commands.md:5179 | `incident start --until-pr` | object | |
| command | commands.md:5591 | `review submit --pr <url>` | pull request | same flag name as the object's |
| commands mentioned | commands.md:4734, 5594, 5529 | `pr post`, `pr merge`, `pr add` | pull request, pull request, object | `pr merge` acts on the pull request |
| error code | commands.md:4793, 5194 | `pr_not_found` | object | |
| error code | commands.md:5195 | `until_pr_not_incident` | object | |
| message | commands.md:5510-5512 | "The clean-up of the <status> PR <pr> of Plan <plan>" (3 messages) | object | text the User reads |
| Event field | commands.md:5514 | `details` `{plan, pr, status, outcome, path, reason}` | object | |
| Event kind `pr-started` | none in the part | | | `rg -n 'pr-started' commands.md` hits only 1083, 2146, 3367, 3692 |
| table column | commands.md:4723 | `slack_reactions.pr` | object | |
| table | commands.md:4766 | `pr_posts` | pull request | |

### "The Gate": relation to the object and to the pull request

- `gate finish` (commands.md:5274-5288) names no PR: it is the Gate's leak check after `scripts/gate-run.sh`, on what a run left (processes, tmux sockets, `$TMPDIR` paths).
- The Gate as the `pre-merge` hook is defined outside this part (CONTEXT.md:235): it runs on a PR's Lead Branch, at the submit of the open or update Job and in `pr open`, `pr push`, `pr rebase`, only when the policy sets `pr.gate = "local"`. So the Gate belongs to the object (Workflow): it checks its Lead Branch before the pull request is opened or pushed. The only other Gate line in this part is commands.md:5580, which names no PR.
- The policy example (commands.md:4808) puts the gate setting at `gate` then `pr`; CONTEXT.md:268 says `pr.gate`. Which is right: unverified.

## Fix list

Judgement rows only. Mechanical `object` rewrites are the rule below, not rows.

**Rule (mechanical, `object` -> Workflow, no row each):** every `object` token in `## PR occurrences` becomes Workflow, in prose and identifiers: `policy show` 13 tokens; `incident` 24; `prune` 3; `janitor` 28 (including the three Event messages at 5510-5512 and the `pr` JSON key at 5487, 5493, 5499 and 5514); `resource` 1; `inbox` 6 (the `slack_reactions.pr` column at 4723 needs a migration decision: applied migrations keep their words). Total 75 tokens.

| path:line | what to change | why |
|---|---|---|
| commands.md:4796 | decide where `pr.post` and the other keys of Domain `pr` go | mixed: a Domain named `pr` covers the object and the pull request |
| commands.md:4808, 4810 | redo the JSON example (Domain `gate` with key `pr`, Domain `pr`) | mixed, and differs from CONTEXT.md:268 (`pr.gate`); right shape unverified |
| commands.md:5186 | "ends when this PR merges" -> "when this Workflow's pull request merges" | mixed: object key, pull request merge |
| commands.md:5466, 5600 | decide the names `core/src/pr/` and `cpd_core::pr::reactions` | mixed: the area covers the pull request and the object |
| commands.md:5591 and 4775 | decide the flag of `review submit` (`--pr <url>`) while `policy show --pr` becomes `--workflow` | one flag name, two meanings today; the User decides |
| commands.md:5313, 5316 | keep "PR" but say "merged pull request" | the same part uses "PR" for the object at 5304 and for the pull request here |
| commands.md:5463 | "pending PR" -> a neutral term such as "Workflow to clean" | loose: `pending` is also an outcome value (5502) |
| commands.md:4515 | write "GitHub deployment run" for the `deploy` item | overloaded: Deploy, run, approval |
| commands.md:4684, 4690, 4717, 4734 | E16 -> E17 for the reaction queue | wrong: 4719 and 4721 say E17; E16 is the janitor (5449) |
| commands.md:4863, 5172, 5274, 5291, 5330, 5370, 5403, 5443 | renumber the headings (`## 23.` six times, then 24, 29, 30, 31) | wrong numbering; it breaks the references below |
| commands.md:4952, 4953, 4982, 5018, 5041, 5053, 5086, 5121 | `§24.0`, `§24.3` -> the real section of `secret` | stale cross-reference (the section is numbered 23) |
| commands.md:5457, 5519, 5520 | `prune, §27` -> the real section | stale: `prune` is numbered 23 |
| commands.md:5594 | check "`pr merge`, §21.9" and "`pr merge` (§9.10)" | §21.9 (4719) is the reaction queue, not `pr merge`; §9.10 unverified |
| commands.md:4830 | name the `deploys` key `workflows` as GitHub Actions workflows in prose | overloaded once Workflow returns |
| commands.md:4721 | "Plan pr-post, PR `react`", "PR `claim`" | the Plan id is history and stays; the "PR" before `react` and `claim` is `object` and changes |
| commands.md:5174 | decide whether "hold" needs a glossary entry (incident, Draft, E17 holding) | loose: three uses of "hold" |

## Counts

- Files audited: 1 (`docs/commands.md`), lines 4513-5632, 1120 lines read, in full.
- Rows: `## Terms` 29 (28 terms and one summary line); `## PR occurrences` 9 individual rows, 17 grouped rows, 19 rows in the Inbox/policy/command table, 3 bullets for the Gate; `## Fix list` 16 rows plus the rule.
- Tokens per sense code (manual tally; method: every token on the lines found by the `rg` commands above): object 75; pull request 32; mixed 6; other 2; phase 0; step 0. Total 115.
- Object per section: inbox 6; policy 13; incident 24; prune 3; janitor 28; resource 1; secret, gate, tag and version, test, mcp 0.
- Pull request per section: inbox 18; incident 2; prune 2; janitor 2; review 8.
- Mixed: policy 3 (4796, 4808, 4810); incident 1 (5186); janitor 1 (5466); review 1 (5600).
- Other: inbox 1 (`pr-post`, 4721); prune 1 (`gh pr list`, 5323).
- Cross-check by `rg`, not the same unit: uppercase `PR`/`PRs` 45 tokens on 26 lines and `pull request` 12 tokens; the remaining tokens are lowercase `pr` identifiers.
