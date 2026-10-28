# Glossary audit: contract sections 7 to 10 (git, Gate, config, policy)

Audit of `docs/contract.md`; report only, no file other than this one was changed. Attempt `aud-contract-4-a1`, Job `aud-contract-4`. Line numbers are those of the file at the base commit of this Attempt (the heading `## 7. git and the Gate` is at line 3491, not at the "about 3466" of the Spec; the file ends at line 4258).

## Coverage

| File | Line range | What I did |
|---|---|---|
| `docs/contract.md` | 3491-3504 (§7 intro) | read in full |
| `docs/contract.md` | 3505-3550 (§7.1 worktrees and branches, §7.2 creating worktrees) | read in full |
| `docs/contract.md` | 3551-3600 (§7.3 hooks) | read in full |
| `docs/contract.md` | 3601-3675 (§7.4 the merge, §7.5 the tamper check) | read in full |
| `docs/contract.md` | 3676-3765 (§7.6 pull requests and the PR rebase) | read in full |
| `docs/contract.md` | 3766-3818 (§7.7 Job kinds) | read in full |
| `docs/contract.md` | 3819-3944 (§8.1 file and format, §8.2 keys) | read in full |
| `docs/contract.md` | 3945-4120 (§8.3 policies, all sub-headings) | read in full |
| `docs/contract.md` | 4121-4159 (§8.4 secrets) | read in full |
| `docs/contract.md` | 4160-4181 (§9 tiers) | read in full |
| `docs/contract.md` | 4182-4258 (§10 traceability, 77 table rows) | read in full; rows 4247-4258 were first printed cut at 400 characters and then the rest of each line (characters 400 onwards) was read in a second command, so every character was read |

Also read, for the glossary: `CONTEXT.md` lines 13-32, 65-80, 107-114, 235-250, 263-273, 298-300 (Phases, Deploy stage, Plan, PR, Gate, Quick check, Lead Branch, Merged, Policy, Domain, Scope, Skipped). Not read: the rest of `CONTEXT.md`; every other line of `docs/contract.md` (other parts' scope); `docs/requirements.md` (cited by the part, so the row for 'requirements' is unverified); `docs/commands.md`; the source code (no sense in the part was checked against code: unverified).

What was only skimmed: nothing in the part. What is **not** individually verified: each of the 444 occurrences of the word `PR`/`PRs` is classified by the dominant reading of its section (`object`), except the ones the regexes below separate (`PR poll`, `PR post`) and the ones listed one by one in the individual table; I did not re-read each of the 444 for a hidden `mixed`.

The part has 768 lines (3491 to 4258). Commands behind the counts (all run from the repo root; `S` and `E` are the first and last line of a range; ranges are in the tables):

```
sed -n 'S,Ep' docs/contract.md | rg -o '\bPRs?\b' | wc -l                  # word PR/PRs per range
sed -n 'S,Ep' docs/contract.md | rg -oi 'pull requests?' | wc -l            # phrase pull request(s)
sed -n 'S,Ep' docs/contract.md | rg -oi 'workflow[a-z_]*' | wc -l           # workflow, workflows, Workflow(s), workflow_runs
sed -n '3491,4258p' docs/contract.md | rg -o -i 'workflow worktrees?' | wc -l   # 41
sed -n 'S,Ep' docs/contract.md | rg -o -- '<token regex>' | wc -l            # lowercase tokens, see below
```

Token regexes (each counted per range with `rg -o ... | wc -l`; the generator that wrote this file is the same list, it is not kept in the repo):

- mixed: `\bpr (open|push|rebase|view)\b`, `\bpr-(opened|merged|base-changed)\b`, `\bpr_(number|url|opened_at|title_required)\b`, `gate\.pr`, `\[pr\]`, `` `pr` ``, `\bpr\.gate`
- object: `--pr\b`, `\bpr (add|start|edit|cancel|describe)\b`, `\bprs\b`, `\bpr-(started|added|title)\b`, `\bpr_(needs|not_open|start_failed|open_escalate_minutes)\b`, `jobs\.pr`, `pr\.jira_required`
- pull request: `\bpr (poll|post|reply|merge)\b`, `\bpr-(feedback|ready|post|posts|poll|comments|base)\b`, `\bpr_(posts?|poll_[a-z_]+|closed)\b`, `\bpr\.(post|feedback)[a-z_.*]*`

Totals by command: `\bPRs?\b` 444, `pull requests?` 109, `workflow` family 65 (50 `workflow`, 1 `Workflow`, 1 `workflow_runs`, 12 `workflows`, 1 `Workflows`), measured with the commands above over lines 3491-4258.

Meaning of the 65 'workflow' uses (by reading each of the lines `rg -n -i workflow` lists): 43 = the PR's Lead worktree ("workflow worktree", 41; "workflow and parallel Attempt worktrees", 1; "the workflow's own git command", 1); 19 = GitHub Actions workflow (lines 3715-3717, 3974, 4003-4007, 4255); 2 = cpd 1's Plan, as history (lines 4192, 4238); 1 = a generic process (line 4233). None means the new Workflow.

No stale `PR` **phase** use exists in the part: every Plan phase named in it is `dispatch`, `deploy`, `done` or `grounding → plan` (lines 3696, 3700, 3705, 3713, 3815, 4205). There is no use of the 'PR' **step** as a named step; the one closest is line 4209 (listed as `mixed`).

## Terms

| Term | Where (path:line) | Sense as used here | Collision | Verdict | Proposed wording |
|---|---|---|---|---|---|
| PR (as the cpd object) | docs/contract.md:3501 "Everything in this section is per PR" | object | internal: PR = the pull request in GitHub-facing keys (`pr poll`, `pull_requests`) and the Plan phase/step 'PR' of CONTEXT.md | overloaded | Workflow (CONTEXT.md decision pro-d136/pro-d138); 'PR' only for the pull request |
| workflow worktree (= Lead worktree) | docs/contract.md:3509 "Workflow worktree, one per PR" | object (the Lead worktree, in other words) | internal: the retired cpd 1 Workflow (= Plan) and the new Workflow; upstream: GitHub Actions workflow; the contract also says 'Lead worktree' for the same directory (43 uses of 'workflow' vs the glossary term 'Lead worktree' at the same place) | overloaded | Use one name. Proposal: 'Lead worktree' (the CONTEXT.md term, already used in the same cell); 'Workflow worktree' would also read right after the rename but is a second name for one thing. The User decides |
| workflow (GitHub Actions workflow) | docs/contract.md:3715 "[deploys] workflows" | other: GitHub Actions workflow file names | internal: the new Workflow; upstream: GitHub Actions workflow | stale | Write 'GitHub Actions workflow' in full in prose; whether the key `[deploys] workflows` keeps its name is the User's decision (a key named `workflows` will read as the cpd Workflow). Candidate: `[deploys] actions_workflows` |
| workflow (JSON key and API path of the deploy cache) | docs/contract.md:3716 "A repo with no declared workflows contributes none" | other: GitHub Actions workflow file (the row's cache key `workflow`, line 3716 earlier in the same sentence) | internal: the new Workflow | stale | Same as above; the cache key `workflow` of `run/deploy-runs.json` would be a Workflow key by the new glossary. Candidate: `actions_workflow` |
| Workflow (cpd 1 Plan) | docs/contract.md:4192 "R7 running Workflows finish on TypeScript cpd" | other: cpd 1's name for Plan (history) | internal: the new Workflow | ok | History row (decision R7 stays as written); add nothing. The CONTEXT.md note that cpd 1's Workflow is retired must not be read as covering the new meaning |
| workflow (cpd 1 Plan, in a decision row) | docs/contract.md:4238 "D3 `workflow` becomes `plan`" | other: cpd 1's Plan (history) | internal: the new Workflow | ok | History: keep. Reviewers should know the sentence is true of cpd 1 only |
| workflow (generic process) | docs/contract.md:4233 "The workflow amended to the User's answers" | other: the process of the Job kinds (not an entity) | internal: the new Workflow; upstream: GitHub Actions workflow | loose | History row; if reworded, say 'The Job-kind flow'; leave as history |
| Plan phase / PR as a Plan phase | docs/contract.md:3696 "the Plan `dispatch → deploy` with `plan-phase`" | other: Plan phase names (grounding, plan, dispatch, deploy, done) | internal: PR status `dispatch` (same word, other entity); Deploy stage; domain `deploy`; Inbox kind `deploy` | overloaded | No stale 'PR' phase in the part: every Plan phase here is already `dispatch` or `deploy`. The word 'dispatch' and 'deploy' each name 2 or more things; see the next rows |
| dispatch (PR status vs Plan phase) | docs/contract.md:3694 "the PR must be `dispatch`" | object status `dispatch` | internal: Plan phase `dispatch`, the Dispatch column of the cockpit, Dispatching step | overloaded | After the rename 'the Workflow must be `dispatch`' still collides with the Plan phase `dispatch`; a status name that differs from the phase's is the User's call (not proposed here) |
| Dispatch (cockpit column) | docs/contract.md:4212 "not part of the Dispatch column" | other: the cockpit's Dispatch column | internal: Dispatch phase | loose | Check against CONTEXT.md in the Cockpit part; not decided here |
| deploy / deploys / Deploy | docs/contract.md:4003 "The existing text-only domain `deploy` (singular, for the Lead) is unrelated and unchanged." | other: policy domain `deploy` (text for the Lead) and `deploys` (GitHub Actions workflows waiting for approval) | internal: Plan phase `deploy`, Deploy stage, Inbox kind `deploy`; upstream: GitHub deployment / environment approvals | overloaded | Four meanings of 'deploy' in one document; the text itself calls two of them 'unrelated'. Proposal: keep the Plan phase and stage as they are; the User decides whether domain `deploys` is renamed (e.g. `approvals`) |
| Gate | docs/contract.md:3560 "**the Gate** for `gate.merge`" | step: hook `pre-commit` called the Gate at the merge | internal: Quick check (CONTEXT.md: the `pre-commit` hook run when a Worker submits); Gate (CONTEXT.md: the `pre-merge` hook); mechanical checks; upstream: git's own `pre-commit` and `pre-merge` hooks (worktrunk 0.80.0, per the text) | wrong | Contradiction inside the part: §7.3 calls `pre-commit` the Gate for `gate.merge` (line 3557) but also the quick check at a Worker's submit (3553) and says submit runs no quick check (3660). Reconcile once, then use Quick check = `pre-commit` and Gate = `pre-merge`, as CONTEXT.md says (or change CONTEXT.md) |
| merge / merged / Merged | docs/contract.md:3619 "End the Attempt `merged` / `merged`" | other: Job and Attempt terminal state (Merged) | internal: PR status `merged`; pull request merged on GitHub (`pr merge`, `pr-merged`); base merge (`git merge origin/<base>`); `merge-tree`; upstream: git merge, GitHub squash merge | overloaded | Five meanings. Proposal: 'Merged' for the Job only (CONTEXT.md); say 'base merge' for merging the base into a branch; 'the pull request merges' for GitHub. Not decided here |
| open / Open Job / `open` | docs/contract.md:3700 "the PR `open` with `pr_number`" | object status `open` (also the `open` Job kind, the pull request state OPEN, the verb in `pr open`) | internal: Open Job (kind `open`), Plan phase; upstream: GitHub pull request state OPEN | overloaded | After the rename 'the Workflow `open`' sits next to the `open` Job and the `OPEN` pull request. Keep, but write 'Open Job' (capitalised, CONTEXT.md) when it is the Job |
| draft / Draft | docs/contract.md:3760 "Ready, never a draft" | pull request: a GitHub draft pull request (the section title)  | internal: Draft (CONTEXT.md: a Job status before the Plan is locked); `draft` Job status in §7.6 ('every Job ... `draft`, `queued` or `running`') | overloaded | Two unrelated drafts. Proposal: 'draft pull request' in full for GitHub's, 'Draft' (capital) for the Job |
| ready | docs/contract.md:3760 "Ready, never a draft" | pull request: not a draft | internal: Ready Job (§3.2); `pr-ready` Inbox item; upstream: `gh pr ready` | overloaded | Three meanings; 'Ready' (capital) is the Job. Say 'a non-draft pull request' where the pull request is meant |
| check / checks | docs/contract.md:3660 "all checks are the tamper check and a `test` or `impl` Job's kind check" | other: Mechanical checks at submit | internal: pull request `checks` (CI), Quick check, Gate, base check, kind check, check span; upstream: GitHub check runs | overloaded | Use 'Mechanical checks' (CONTEXT.md) for the submit checks, 'CI checks' for the pull request's `checks` |
| run (workflow run, hook run, `run` command) | docs/contract.md:3715 "`workflow_runs[]` carry" | other: GitHub Actions run | internal: `run` in `[test-commands]`, `cpd resource run`, `run/<plan>/` directory | overloaded | Write 'GitHub Actions run' in full; the other three are cpd's own and are unambiguous in place |
| lock / locked | docs/contract.md:3503 "the driver lock" | other: flock file | internal: `plan lock`, Locked (Job, CONTEXT.md); upstream: `git worktree add --lock`, `index.lock`, `config.lock`, ref locks | overloaded | Seven meanings across §7. Keep 'Locked' (capital) for the Job; say 'file lock' / 'git lock' when it is the flock or git's |
| review / Review Job / review_decision | docs/contract.md:3650 "A review Job's merge" | other: Review Job | internal: Reviewing step; upstream: GitHub pull request review (`reviewDecision`, `reviews`) | overloaded | Capitalise 'Review Job' for the Job; 'pull request review' for GitHub's |
| status vs state | docs/contract.md:3700 "`state` `open`" | pull request: GitHub `state` field | internal: PR `status` (waiting, dispatch, open, merged, cancelled), Job status, Attempt status; Session state | loose | Two words for the same idea on two objects (`prs.status`, `pull_requests.state`); not decided here |
| waiting | docs/contract.md:3715 "status=waiting" | other: GitHub Actions run status `waiting` | internal: PR status `waiting` (§7.6, 'waiting' dependents) | overloaded | Write 'GitHub Actions run status waiting' in full; keep Workflow status `waiting` |
| cancel / cancelled / CANCELLED | docs/contract.md:3710 "`CANCELLED` is neither failed nor pending" | pull request: check conclusion CANCELLED | internal: PR/Job/Attempt `cancelled`; upstream: GitHub check conclusion | overloaded | Fine in place (upper case is GitHub's); keep |
| step / driver step A8 / rule letters | docs/contract.md:3601 "driver step A8" | other: algorithm step of the merge | internal: CONTEXT.md steps (Mechanical checks, PR step, Reviewing...); decision ids A1..A14 and R1..R36 (e.g. 'R4' at line 3493 is a decision, 'R4.' at 3643 is a recovery step); rule S vs Tier S; G (commit) vs Tier G | overloaded | Rename the merge procedure's labels so they cannot be read as decision ids or Tier letters (for example numbers only). The User decides |
| session (cpd, tmux, pi) | docs/contract.md:3941 "`tmux.cockpit_session`" | other: tmux session names | internal: Session (CONTEXT.md); upstream: tmux session, pi session | overloaded | Write 'tmux session' for the config keys; 'Session' (capital) for cpd's. Keep |
| tier / Tier | docs/contract.md:4245 "The Inbox notice tier" | other: Inbox urgency tier | internal: Tier (model and effort, CONTEXT.md and §9) | overloaded | 'urgency tier' for the Inbox; 'Tier' only for model+effort |
| owner / owns / owned | docs/contract.md:3707 "a Plan owns" | other: a Plan 'owns' a pull request through its PR | internal: Context `owner` (GitHub login), Known owner; upstream: GitHub repository owner | loose | 'is the pull request of a Workflow' instead of 'owns' (line 3707 and its siblings); keep 'owner' for the GitHub login |
| requirements (docs/requirements.md) | docs/contract.md:3539 "(requirements §5, kept)" | other: a requirements document, unverified which | internal: Requirements (CONTEXT.md Grounding output) | loose | Name the file ('docs/requirements.md', unverified: not read) instead of 'requirements §n' |
| message (verb and noun) | docs/contract.md:3680 "message the Worker" | other: sending a message to a Worker Session | internal: Event, Inbox item, Brief; upstream: Slack message (`pr post`) | loose | Fine; say 'send a message' once for the reader |
| post / `pr post` / `pr.post` | docs/contract.md:3964 "`post.enabled = true" | pull request: posting a pull request to Slack | internal: `pr reply` posts a comment; upstream: Slack post, HTTP POST | loose | Keep 'PR post' as the CONTEXT.md term; 'post a comment' for `pr reply` |
| Skipped (the step) | docs/contract.md:3613 "Skipped when `old` is already an ancestor" | other: a step of the merge is skipped | internal: Skipped (CONTEXT.md: a Ready Job E3 could not start) | loose | 'Not run when' avoids the capital-S term; minor |
| scope(s) | docs/contract.md:3947 "Policies have three scopes" | other: policy Scope | internal: CONTEXT.md Scope (four: default, machine, owner, repo) | wrong | Text says 'three scopes' and lists four; reconcile with CONTEXT.md Scope (four) — see Fix list |
| context (hook JSON) | docs/contract.md:3577 "the JSON context on stdin" | other: JSON object passed to a hook | internal: Context (entity) | loose | The lower-case generic 'context' is fine; no change |

Fine, no row: Plan, Plan phase (names only), Context (entity), Context record, Context store, Job, Job kind, Update Job, Spec, Dependency, Attempt, Ledger, Locked, Event, Inbox, Inbox item, Engine, Feedback item, PR poll, PR post, User, Manager, Lead, Worker, Escalation, Lead Branch, Merged (as the Job state), Tier (as model and effort), Axiom, Policy, Domain, Scope (apart from the count row), Exposure, Policy key, Resource, MCP server, Resolved policy, Known owner. Absent from the part, so not assessed: Plan stage, Distilled Context, Grounding, Clarifying, Specifying, Sequencing, Approving, Dispatching, Mechanical checks (term), Reviewing, Monitoring, Addressing, Brief, Pending work, Context update, Quick check (capitalised), Repo policy file (as that phrase), Slot, Tenant, Machine policy (as that phrase), Attach, Fallback. Every other word to watch (task, ticket, epic, board, record, decision, note, outline, finding, assumption, hold, park, archive, queue, block, stop, fail, push, poll, branch, base, event) occurs only in its plain sense or is covered by the rows above; a count was not made for each (unverified).

## PR occurrences

Method: counts come from the `rg`/regex commands in Coverage, run on each section range; the sense of a group is the sense of its text after reading every line of the part; a word the regex cannot tell apart is listed in the individual rows below. Row groups: first the counts by section and sense, then the individual `mixed`, `step`, `phase` and `other` uses.

| Where (section or path:line range) | Count | Sense | Quote | After the rename |
|---|---|---|---|---|
| docs/contract.md:3491-3504 (§7 intro), word PR/PRs | 13 | object | "Everything in this section is per PR" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3499 (first of 2 in §7 intro) | 2 | mixed | "pr open" | see rule M in Fix list |
| docs/contract.md:3499 (first of 2 in §7 intro) | 2 | mixed | "pr rebase" | see rule M in Fix list |
| docs/contract.md:3501 (first of 1 in §7 intro) | 1 | mixed | "pr view" | see rule M in Fix list |
| docs/contract.md:3501 (first of 3 in §7 intro), tokens | 3 | object | "--pr" | Workflow rename (mechanical) |
| docs/contract.md:3505-3519 (§7.1), word PR/PRs | 22 | object | "A PR made by migration 4 from a Plan of before R34" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3516 (first of 1 in §7.1) | 1 | mixed | "pr rebase" | see rule M in Fix list |
| docs/contract.md:3509 (first of 1 in §7.1), tokens | 1 | object | "pr start" | Workflow rename (mechanical) |
| docs/contract.md:3520-3550 (§7.2), word PR/PRs | 45 | object | "Lead worktree of an adopted PR" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3520-3550 (§7.2), phrase 'pull request(s)' | 7 | pull request | "the pull request's recorded head accepts them" | unchanged |
| docs/contract.md:3526 (first of 2 in §7.2) | 2 | mixed | "pr open" | see rule M in Fix list |
| docs/contract.md:3545 (first of 1 in §7.2) | 1 | mixed | "pr view" | see rule M in Fix list |
| docs/contract.md:3523 (first of 3 in §7.2), tokens | 3 | object | "pr start" | Workflow rename (mechanical) |
| docs/contract.md:3551-3600 (§7.3), word PR/PRs | 8 | object | "after a workflow worktree is created at the PR's start" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3560 (first of 2 in §7.3) | 2 | mixed | "pr open" | see rule M in Fix list |
| docs/contract.md:3560 (first of 2 in §7.3) | 2 | mixed | "pr push" | see rule M in Fix list |
| docs/contract.md:3560 (first of 2 in §7.3) | 2 | mixed | "pr rebase" | see rule M in Fix list |
| docs/contract.md:3597 (first of 1 in §7.3) | 1 | mixed | "pr_title_required" | see rule M in Fix list |
| docs/contract.md:3561 (first of 2 in §7.3) | 2 | mixed | "gate.pr" | see rule M in Fix list |
| docs/contract.md:3597 (first of 1 in §7.3), tokens | 1 | pull request | "pr_closed" | unchanged |
| docs/contract.md:3601-3661 (§7.4), word PR/PRs | 24 | object | "no other Attempt of its PR is live when inserted" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3601-3661 (§7.4), phrase 'pull request(s)' | 9 | pull request | "the pull request is created or adopted" | unchanged |
| docs/contract.md:3617 (first of 8 in §7.4) | 8 | mixed | "pr open" | see rule M in Fix list |
| docs/contract.md:3617 (first of 5 in §7.4) | 5 | mixed | "pr push" | see rule M in Fix list |
| docs/contract.md:3616 (first of 9 in §7.4) | 9 | mixed | "pr rebase" | see rule M in Fix list |
| docs/contract.md:3657 (first of 1 in §7.4) | 1 | mixed | "pr_title_required" | see rule M in Fix list |
| docs/contract.md:3614 (first of 3 in §7.4) | 3 | mixed | "gate.pr" | see rule M in Fix list |
| docs/contract.md:3655 (first of 1 in §7.4), tokens | 1 | object | "prs" | Workflow rename (mechanical) |
| docs/contract.md:3662-3675 (§7.5), word PR/PRs | 13 | object | "each path in the PR's `protected.paths`" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3676-3765 (§7.6), word PR/PRs | 164 | object | "Each PR has its own Gate run" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3676-3765 (§7.6), phrase 'pull request(s)' | 74 | pull request | "Each PR has its own Gate run and its own pull request" | unchanged |
| docs/contract.md:3682 (first of 23 in §7.6) | 23 | mixed | "pr open" | see rule M in Fix list |
| docs/contract.md:3682 (first of 14 in §7.6) | 14 | mixed | "pr push" | see rule M in Fix list |
| docs/contract.md:3680 (first of 38 in §7.6) | 38 | mixed | "pr rebase" | see rule M in Fix list |
| docs/contract.md:3718 (first of 2 in §7.6) | 2 | mixed | "pr view" | see rule M in Fix list |
| docs/contract.md:3696 (first of 2 in §7.6) | 2 | mixed | "pr-opened" | see rule M in Fix list |
| docs/contract.md:3696 (first of 3 in §7.6) | 3 | mixed | "pr-merged" | see rule M in Fix list |
| docs/contract.md:3695 (first of 3 in §7.6) | 3 | mixed | "pr-base-changed" | see rule M in Fix list |
| docs/contract.md:3696 (first of 3 in §7.6) | 3 | mixed | "pr_number" | see rule M in Fix list |
| docs/contract.md:3696 (first of 2 in §7.6) | 2 | mixed | "pr_url" | see rule M in Fix list |
| docs/contract.md:3696 (first of 2 in §7.6) | 2 | mixed | "pr_opened_at" | see rule M in Fix list |
| docs/contract.md:3694 (first of 1 in §7.6) | 1 | mixed | "pr_title_required" | see rule M in Fix list |
| docs/contract.md:3690 (first of 10 in §7.6) | 10 | mixed | "gate.pr" | see rule M in Fix list |
| docs/contract.md:3713 (first of 1 in §7.6) | 1 | mixed | `pr` | see rule M in Fix list |
| docs/contract.md:3734 (first of 1 in §7.6) | 1 | mixed | "pr.gate" | see rule M in Fix list |
| docs/contract.md:3678 (first of 32 in §7.6), tokens | 32 | object | "--pr" | Workflow rename (mechanical) |
| docs/contract.md:3678 (first of 35 in §7.6), tokens | 35 | pull request | "pr poll" | unchanged |
| docs/contract.md:3766-3818 (§7.7), word PR/PRs | 26 | object | "the PR has a stored `title`" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3766-3818 (§7.7), phrase 'pull request(s)' | 6 | pull request | "the newest pull request of the Lead Branch is not" | unchanged |
| docs/contract.md:3808 (first of 8 in §7.7) | 8 | mixed | "pr open" | see rule M in Fix list |
| docs/contract.md:3817 (first of 1 in §7.7) | 1 | mixed | "pr push" | see rule M in Fix list |
| docs/contract.md:3817 (first of 1 in §7.7) | 1 | mixed | "pr rebase" | see rule M in Fix list |
| docs/contract.md:3815 (first of 2 in §7.7) | 2 | mixed | "pr-opened" | see rule M in Fix list |
| docs/contract.md:3815 (first of 1 in §7.7) | 1 | mixed | "pr-merged" | see rule M in Fix list |
| docs/contract.md:3815 (first of 1 in §7.7) | 1 | mixed | "pr_number" | see rule M in Fix list |
| docs/contract.md:3815 (first of 1 in §7.7) | 1 | mixed | "pr_url" | see rule M in Fix list |
| docs/contract.md:3815 (first of 1 in §7.7) | 1 | mixed | "pr_opened_at" | see rule M in Fix list |
| docs/contract.md:3808 (first of 1 in §7.7) | 1 | mixed | "pr_title_required" | see rule M in Fix list |
| docs/contract.md:3780 (first of 3 in §7.7), tokens | 3 | object | "pr add" | Workflow rename (mechanical) |
| docs/contract.md:3817 (first of 2 in §7.7), tokens | 2 | pull request | "pr-feedback" | unchanged |
| docs/contract.md:3882 (§8.1-§8.2), word PR | 1 | pull request | "every open PR of the User" | unchanged |
| docs/contract.md:3819-3944 (§8.1-§8.2), word PR/PRs | 1 | object | "escalation of an unopened PR to the Manager" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3819-3944 (§8.1-§8.2), words 'PR poll' / 'PR post' | 1 | pull request | "of the PR poll may fail" | unchanged (CONTEXT.md terms PR poll, PR post keep PR = pull request) |
| docs/contract.md:3943 (first of 1 in §8.1-§8.2), tokens | 1 | object | "pr_open_escalate_minutes" | Workflow rename (mechanical) |
| docs/contract.md:3882 (first of 4 in §8.1-§8.2), tokens | 4 | pull request | "pr_poll_seconds" | unchanged |
| docs/contract.md:3945-4120 (§8.3), word PR/PRs | 25 | object | "a PR, the Workers of its Jobs" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:3945-4120 (§8.3), words 'PR poll' / 'PR post' | 4 | pull request | "The PR poll (§7.6) and engine E8" | unchanged (CONTEXT.md terms PR poll, PR post keep PR = pull request) |
| docs/contract.md:3945-4120 (§8.3), phrase 'pull request(s)' | 2 | pull request | "h; `default (no owner file)` for a pull request row no PR owns whos" | unchanged |
| docs/contract.md:3964 (first of 5 in §8.3) | 5 | mixed | "pr open" | see rule M in Fix list |
| docs/contract.md:4013 (first of 4 in §8.3) | 4 | mixed | "pr push" | see rule M in Fix list |
| docs/contract.md:4013 (first of 5 in §8.3) | 5 | mixed | "pr rebase" | see rule M in Fix list |
| docs/contract.md:3963 (first of 8 in §8.3) | 8 | mixed | "[pr]" | see rule M in Fix list |
| docs/contract.md:3963 (first of 6 in §8.3) | 6 | mixed | `pr` | see rule M in Fix list |
| docs/contract.md:4013 (first of 1 in §8.3) | 1 | mixed | "pr.gate" | see rule M in Fix list |
| docs/contract.md:4065 (first of 21 in §8.3), tokens | 21 | object | "--pr" | Workflow rename (mechanical) |
| docs/contract.md:4110 (first of 12 in §8.3), tokens | 12 | pull request | "pr poll" | unchanged |
| docs/contract.md:4160-4181 (§9), word PR/PRs | 1 | object | "Plan bifrost, PR catfp" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:4256 (§10), word PR | 1 | pull request | "the cockpit's PRs section" | unchanged |
| docs/contract.md:4182-4258 (§10), word PR/PRs | 85 | object | "R11 one big Plan, one PR" | Workflow (mechanical rewrite, rule below) |
| docs/contract.md:4182-4258 (§10), words 'PR poll' / 'PR post' | 5 | pull request | "PR post" | unchanged (CONTEXT.md terms PR poll, PR post keep PR = pull request) |
| docs/contract.md:4182-4258 (§10), phrase 'pull request(s)' | 11 | pull request | "Authored pull requests in the Inbox" | unchanged |
| docs/contract.md:4201 (first of 5 in §10) | 5 | mixed | "pr open" | see rule M in Fix list |
| docs/contract.md:4215 (first of 4 in §10) | 4 | mixed | "pr push" | see rule M in Fix list |
| docs/contract.md:4201 (first of 4 in §10) | 4 | mixed | "pr rebase" | see rule M in Fix list |
| docs/contract.md:4201 (first of 5 in §10) | 5 | mixed | "pr view" | see rule M in Fix list |
| docs/contract.md:4201 (first of 2 in §10) | 2 | mixed | "pr-base-changed" | see rule M in Fix list |
| docs/contract.md:4231 (first of 1 in §10) | 1 | mixed | `pr` | see rule M in Fix list |
| docs/contract.md:4200 (first of 18 in §10), tokens | 18 | object | "--pr" | Workflow rename (mechanical) |
| docs/contract.md:4201 (first of 25 in §10), tokens | 25 | pull request | "pr poll" | unchanged |

Individual `mixed`, `step`, `phase` and `other` uses (judgement rows; counts above include them for the `mixed` tokens, not for the words):

| Where (path:line) | Count | Sense | Quote | Why | After the rename |
|---|---|---|---|---|---|
| docs/contract.md:3676 | 1 | mixed | "Pull requests and the PR rebase" | The heading names the pull request and `pr rebase`, which rebases a Workflow's Lead Branch and pushes the pull request branch: both | Pull requests and the Workflow rebase (User decides) |
| docs/contract.md:3694 | 1 | mixed | "the way a PR opens" | The PR's `open` Job opens its pull request: the object and the pull request in one phrase | the way a Workflow's pull request opens |
| docs/contract.md:3694 | 1 | mixed | "`pr open --pr <key>`" | Acts on a Workflow (key) and opens its pull request; the noun may stay 'pr' (pull request) with the flag renamed | `cpd pr open --workflow <key>` (proposal; User decides) |
| docs/contract.md:3753 | 1 | mixed | "`pr push [--pr <key>]`" | Acts on a Workflow's Lead Branch and publishes it to the pull request | `cpd pr push --workflow <key>` (proposal) |
| docs/contract.md:3720 | 1 | mixed | "`pr rebase --pr <key>`" | Rebases a Workflow's Lead Branch and force-pushes the pull request branch | `cpd pr rebase --workflow <key>` (proposal) |
| docs/contract.md:3718 | 1 | mixed | "`pr view [--pr <key>]`" | Key is a Workflow; it prints pull request detail (checks, reviews, comments) | `cpd pr view --workflow <key>` (proposal) |
| docs/contract.md:3743 | 1 | mixed | "`pr rebase --pr <key> --continue`" | Same as `pr rebase` (Workflow key, pull request push) | `--workflow <key>` |
| docs/contract.md:3752 | 1 | mixed | "`pr rebase --pr <key> --abort`" | Same as `pr rebase` | `--workflow <key>` |
| docs/contract.md:3696 | 1 | mixed | "`pr-opened`" | Event: the Workflow becomes `open` and a pull request exists | Event kind `workflow-opened`, or keep `pr-opened` for the pull request (User decides) |
| docs/contract.md:3696 | 1 | mixed | "`pr-merged`" | Event: the pull request merged and the Workflow becomes `merged` | `workflow-merged`, or keep |
| docs/contract.md:3695 | 1 | mixed | "`pr-base-changed`" | Event: the Workflow's `base` follows the pull request's base | `workflow-base-changed` |
| docs/contract.md:3696 | 1 | mixed | "`pr_number`" | Column of the `prs` row that stores the pull request's number (also `pr_url`, `pr_opened_at`, same line) | `pull_request_number` / keep: a Workflow field holding a pull request fact |
| docs/contract.md:3597 | 1 | mixed | "`pr_title_required`" | Error: the Workflow has no stored title for the pull request it will create | `workflow_title_required` or `pull_request_title_required` (User decides) |
| docs/contract.md:3561 | 1 | mixed | "`gate.pr`" | Policy key: the Gate that runs before the pull request opens, set per Workflow | `gate.pull_request` / keep `gate.pr` (pull request): User decides |
| docs/contract.md:3964 | 1 | mixed | "a PR of the scope needs its Jira ticket" | Domain `pr` row: this key is about the Workflow (object) while the same row's `post.enabled` ('enables posting PRs to Slack') and `feedback.checks` are about the pull request; one domain, two senses | Split by sense or rename the domain; User decides |
| docs/contract.md:3984 | 1 | mixed | "Deprecated `[pr] gate` alias." | Old key `[pr] gate` of the same domain: gate of the pull request opening | Same as `gate.pr`; history |
| docs/contract.md:4209 | 1 | mixed | "before the PR opens (`pr open`)" | Traceability R19: the Deploy step 'PR' (pull request is opened) and the Workflow's `open` status | before the pull request opens (`pr open`) |
| docs/contract.md:4253 | 1 | mixed | "no PR Gate" | Gate per Workflow before opening the pull request (CONTEXT.md Gate) | no Gate at the Workflow level |
| docs/contract.md:4173 | 1 | mixed | "the code of this PR only makes" | 'This PR' is the change being implemented: a Workflow of a Plan and the pull request it becomes | this Workflow (or: this pull request) (User decides) |
| docs/contract.md:3722 | 1 | other | "Escalation nl-pr-a1-q1" | An Escalation id; 'pr-a1' is an attempt or job name of a past Plan (history) | unchanged (history) |
| docs/contract.md:3705 | 1 | other | "no phase precondition" | Plan phase in the generic sense; there is no stale 'PR' phase in the part | unchanged |
| docs/contract.md:4200 | 1 | other | "A10 phases and polling" | Decision A10's phases are the Plan phases; the label is history | unchanged |
| docs/contract.md:3655 | 1 | step | "Step 8b, an `open` Job only," | Step of the merge procedure (not the Deploy 'PR' step); it opens the pull request, so a reader may take it for the 'PR' step of CONTEXT.md | Name the label 'open-pull-request' to avoid the clash |

## Fix list

| path:line | what to change | why |
|---|---|---|
| docs/contract.md:3787 "(§7.3 precondition 6)" | §7.3 precondition 6: Point to precondition 5 ('the kind's check'); §7.3 lists five preconditions (3592-3598) | Stale cross-reference: no precondition 6 exists |
| docs/contract.md:3660 "runs no quick check and no Gate" | Quick check vs Gate in §7.3 and §7.7: Reconcile with 3553 ('the quick check at a Worker's submit (`pre-commit`)'), 3557 ('**the Gate** for `gate.merge`') and 3805 ('the quick check (`pre-commit`)'); then use Quick check = `pre-commit`, Gate = `pre-merge` (CONTEXT.md) | Three statements on what `pre-commit` is and whether submit runs it; one says submit runs none; I infer a contradiction (not run) |
| docs/contract.md:3560 "**the Gate** for `gate.merge`" | `pre-commit` named the Gate: Rename the row's 'Used for' or CONTEXT.md; the two glossary terms Gate and Quick check swap meanings here | Collision with CONTEXT.md Quick check |
| docs/contract.md:3859 "context_distill_tier = "H0"" | distiller Tier default, three values: Make the §8.2 example (H0), the §8.2 table (`GX`, line 3912) and §9 Defaults (`H0`, line 4179) agree; §9 line 4173 says only GX is valid | Wrong/stale: the example would be `config_invalid` under hs-d562 |
| docs/contract.md:3947 "Policies have three scopes" | `three scopes`, four named: Say 'four scopes' (default, machine, owner, repo) as CONTEXT.md Scope and the next bullet do | Count disagrees with the list in the same sentence |
| docs/contract.md:4038 "stays the nine above" | `the nine above`: Replace by the actual count or by 'the domains above' (the table at 3962-3975 lists 12 rows, and `model` is a 13th) | Stale count; I counted rows from the table, not from code (unverified) |
| docs/contract.md:4258 "§7.1 `scripts/check-migrations-frozen.sh`" | traceability pointer to §7.1: §7.1 (lines 3505-3519) does not mention the script; `rg` finds it elsewhere only at line 2079 (section not checked); fix the pointer | Stale cross-reference |
| docs/contract.md:3694 "the PR must be `dispatch`" | PR status `dispatch` vs Plan phase `dispatch`: Decide whether the Workflow status `dispatch` keeps its name; then the sentence reads 'the Workflow must be `dispatch`' | Same word names a status of the object and a Plan phase (CONTEXT.md Phases) |
| docs/contract.md:3509 "Workflow worktree, one per PR" | Lead worktree called workflow worktree: Choose 'Lead worktree' (CONTEXT.md) everywhere; rule: 43 uses of 'workflow' in §7 mean the Lead worktree (list: rg -n -i 'workflow' lines 3509-3768); this is a mechanical rule once decided | Second name for the Lead worktree; collides with the new Workflow |
| docs/contract.md:3715 "[deploys] workflows" | [deploys] workflows: Write 'GitHub Actions workflow' in prose (19 uses of 'workflow(s)' in §7.6 step 8 and §8.3 deploys); decide the key name `workflows` and the cache key `"workflow"` | Collides with the new Workflow; spec says a GitHub Actions workflow is always written in full |
| docs/contract.md:4003 "The existing text-only domain `deploy` (singular, for the Lead) is unrelated and unchanged." | deploy / deploys domains: Keep the sentence but add 'not the Plan phase deploy, not the Inbox kind deploy'; or rename `deploys` | Four meanings of 'deploy' (see Terms) |
| docs/contract.md:3601 "driver step A8" | rule/step labels: Rename labels A1..A14, R, S, P, U, G, C0..C6 of the merge so they cannot be mistaken for decision ids and Tier letters | Line 3493 '(R4)' is a decision; line 3643 '**R4.**' is a recovery step |
| docs/contract.md:3707 "a Plan owns" | PR `owns` / owned by: Use 'is the pull request of a Workflow'; 8 places say owns/owned (3707, 3713, 4086...) | Loose: ownership of a pull request vs GitHub owner |
| docs/contract.md:4209 "before the PR opens (`pr open`)" | PR step 'PR' in R19 row: Reword as 'before the pull request opens' | mixed: Deploy step 'PR' vs Workflow status |
| docs/contract.md:4245 "The Inbox notice tier" | Inbox notice tier vs Tier: Say 'urgency tier' in the traceability rows (4245-4250) | 'Tier' is the model Tier in the glossary |

Rules (mechanical, count per section; no row each):

- Rule O: every `object` use of PR/PRs and every object token (`--pr`, `pr add`, `pr start`, `pr edit`, `pr cancel`, `pr describe`, `prs`, `pr-started`, `pr-added`, `pr_needs`, `jobs.pr`, `pr.jira_required`, `pr_not_open`, `pr_start_failed`) becomes Workflow / the Workflow equivalent. Counts by section are the `object` rows of PR occurrences.
- Rule P: every `pull request` use and pull request token (`pr poll`, `pr post`, `pr reply`, `pr merge`, `pr-feedback`, `pr-ready`, `pr_posts`, `pr_poll_*`, `pr_closed`, `pr.post.*`, `pr.feedback.*`) stays.
- Rule M: every `mixed` token form (`pr open`, `pr push`, `pr rebase`, `pr view`, `pr-opened`, `pr-merged`, `pr-base-changed`, `pr_number`, `pr_url`, `pr_opened_at`, `pr_title_required`, `gate.pr`, the `pr` domain and the `[pr]` table) needs one decision by the User per form (noun stays and flag changes, or the whole name changes); the section counts are the `mixed` rows. Proposal only.
- Rule W: every `workflow` that means a GitHub Actions workflow (19 uses, §7.6 step 8 and §8.3 `deploys`) is written 'GitHub Actions workflow'; every one that means the Lead worktree (43 uses, §7.1-§7.4, §7.7) follows the choice in the Lead-worktree row.

## Counts

| What | Number |
|---|---|
| files audited | 1 (`docs/contract.md`); `CONTEXT.md` read for glossary only |
| lines read in the part | 768 (3491-4258), all read in full |
| rows in `## Terms` | 33 (plus one summary paragraph for terms that are fine) |
| rows in `## PR occurrences`, counts by section and sense | 89 |
| rows in `## PR occurrences`, individual `mixed` / `step` / `phase` / `other` | 23 (two of them, `no phase precondition` and `A10 phases and polling`, are uses of the word 'phase', not of PR, and count 0 below) |
| rows in `## Fix list` | 15, plus 4 mechanical rules |
| word `PR`/`PRs` in the part | 444 |
| phrase `pull request(s)` in the part | 109 |
| `workflow` family in the part | 65 (43 Lead worktree, 19 GitHub Actions, 2 cpd 1 Plan, 1 generic) |
| occurrences per sense code (words + lowercase tokens, as the regexes count them) | object 510, pull request 200, mixed 221, other 2, phase 0, step 0 |

How the sense totals are built: `object` = PR/PRs words not otherwise classified + object tokens; `pull request` = `pull request` phrases + `PR poll`/`PR post` words + the two PR words listed in the section rows + pull request tokens; `mixed` = mixed tokens (216) + 5 mixed words listed in the individual table; `other` = the two `pr-a1` tokens of the Escalation ids. The `step` code is used once in the individual table for the `Step 8b` label, which is not a PR use, so it adds 0. These totals are only as good as the regexes (a token written differently would be missed; unverified).
