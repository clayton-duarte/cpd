# Stale-while-revalidate (SWR): the Jira datasource

Status: **normative for the `jira` Workflow** (Plan `external-sync-arch`, Job `swr-jira`). It is the design page that [swr.md §14](swr.md) names for the `jira` source. Where merged code later differs, the code wins. Decisions: pro-d443 (Jira), pro-d126 and pro-d128 (the transitions this replaces). Accepted assumptions cited here are the A-list of [swr.md §17](swr.md). Decisions this page makes on its own are marked **J1** to **J7** and listed in §13 for the Lead.

## 1. Rules

- **JIRA RULES (pro-d443).** Jira stays write-driven. There is **no background read loop**: nothing reads a Jira issue except a transition run. The `jira` source's `due` returns no keys and its `fetch` makes no call, so the generic loop of [swr.md §12](swr.md) does nothing for Jira.
- **One helper.** Every Jira call runs in `pi/lib/jira-transition.ts` on the `Runner` port (A7). The helper does I/O only and reads the Atlassian connection from the machine policy's `[mcp.servers.atlassian]`, passed on stdin (`core/src/jira.rs:55-82`). It never reads `~/.pi/agent/mcp.json`.
- **Time is an argument** ([swr.md §16](swr.md)). Every function here takes `now` or `poll_started_at`.
- **One rank table, one skip rule.** Both live in Rust in `core/src/swr/source/jira.rs`. The helper repeats the skip comparison on its fresh read, from strings Rust passes it (§3.2, J4). No other module restates either.

## 2. Keys, rows and the one op

| Thing | Value |
|---|---|
| query key | `jira:issue:<TICKET>`, e.g. `jira:issue:EEC-1` |
| `swr_queries.system` | `jira` |
| outbox `system`, `target_key`, `op_kind` | `jira`, `jira:issue:<TICKET>`, `transition` |
| `vars_json` | `{"ticket": "EEC-1", "target": "in_review", "rank": 2}` |
| `context_json` | `{"plan": "pay-4521", "workflow": "ui"}`, plus `"refused_with": "<ticket>\|<status>"` once refused |
| `patch_json` | `{"status_name": "In Review", "status_category": "indeterminate"}`, or NULL (§6) |
| endpoint | `jira` (`swr_endpoints`) |
| cache tables | `swr_jira_issues` (one row per ticket), `swr_atlassian` (one row: `cloud_id`, `account_id`, `fetched_at`) |

**J1.** The Job's spelling of the target key is `jira:<TICKET>`. This page writes it `jira:issue:<TICKET>`, so `target_key` is the query key that `complete` invalidates ([swr.md §3.3, §14](swr.md)). One ticket, one spelling.

The only op is `transition`. No other Jira write exists.

**Ticket creation (pro-d454).** cpd never creates a Jira ticket itself. The `cpd-lead` and `cpd-manager` skills require a model that creates one with `mcp__atlassian__createJiraIssue` to call `mcp__atlassian__atlassianUserInfo` first and pass its account id as `assignee_account_id`, so a ticket cpd works is assigned to the User from the start; the helper's assignee write (§3.2 step 3a) keeps it so.

## 3. The helper's JSON contract

One helper run is one spawn: one JSON object on stdin, one JSON line on stdout. The run is detached, under the 120 s deadline of `core/src/jira.rs` (30 s per Jira call, as today).

### 3.1 Request (stdin)

```json
{
  "server": {"url": "https://mcp.atlassian.com/v1/mcp/authv2", "exposure": "deferred"},
  "cloud_id": "b4c1d0e2-…",
  "account_id": "712020:5b1c…",
  "ticket": "EEC-1",
  "target": "in_review",
  "target_name": "In Review",
  "later_names": ["Done"],
  "estimate_fields": ["customfield_10016"]
}
```

| Key | Rule |
|---|---|
| `server` | the machine policy's `atlassian` entry, as today; `url` required |
| `cloud_id` | `swr_atlassian.cloud_id` when `fetched_at` is less than 24 h before `now`; otherwise `null` |
| `account_id` | `swr_atlassian.account_id` when the row is fresh (the same `fetched_at` test as `cloud_id`, and the column is not NULL); otherwise `null`, which means the helper resolves it (§3.2 step 1; pro-d454) |
| `ticket` | `^[A-Z][A-Z0-9]+-[0-9]+$` |
| `target` | `in_progress`, `in_review` or `done` |
| `target_name` | the policy's `pr.jira.<target>`, non-empty |
| `later_names` | the non-empty policy names of the targets of higher rank than `target`, in rank order; `[]` when none (§4). Rust computes it, so the helper holds no rank table |
| `estimate_fields` | the policy's `pr.jira.estimate_fields` when `target` is `done`, otherwise `[]` |

Any other shape is `usage` (exit 2) before the helper connects, as today. The request drops the old `names` object, which carried the names of all three targets.

### 3.2 The helper's calls, in order

1. **Site and user.** If `account_id` is `null`: `atlassianUserInfo {}`; the authenticated user's account id is the result field `account_id`, or `accountId` when that is absent; a result with neither is `jira_failed`. A non-null `account_id` skips this call (pro-d454). If `cloud_id` is `null`: `getAccessibleAtlassianResources {}`. Keep the entries whose `scopes` include `read:jira-work` and `write:jira-work`. None: `jira_auth` (`reason` `no_jira_site`). More than one distinct `id`: `jira_refused` (`site_ambiguous`). Else the one `id` is the `cloud_id`. A non-null `cloud_id` skips this call.
2. **The one read.** `getJiraIssue {cloudId, issueIdOrKey: ticket, fields: ["status", "summary", "assignee", ...estimate_fields]}`. A 404 is `jira_refused` (`issue_not_found`). From the answer: `status_name` = `fields.status.name`, `status_category` = `fields.status.statusCategory.key`, `summary` = `fields.summary`, `assignee` = `fields.assignee?.accountId` (absent when the issue is unassigned), and `estimates` = each estimate field present as a key, with its value (`null` means empty). A key absent from `fields` is not in `estimates`.
3. **The skip test on the fresh read** (§5). Order: `status_name == target_name` → `unchanged`, `why` `at_target`; `status_category == "done"` → `unchanged`, `done_category`; `status_name` in `later_names` → `unchanged`, `past_target`. Only the assignee write of step 3a (never a transition) follows an `unchanged`.
3a. **The assignee** (pro-d454). Whenever `fields.assignee?.accountId` is not `account_id`, the helper sets `fields.assignee = {accountId: <account_id>}` through `editJiraIssue`. This holds on every outcome, an `unchanged` one included (a ticket already at or past the target is still assigned). For `target` `done` that write is coalesced with step 5's estimate write into the one `editJiraIssue` call. A failed assignee edit is `jira_failed` and the ticket is not moved (§3.2 step 6 is not reached).
4. **The transition.** `getTransitionsForJiraIssue {cloudId, issueIdOrKey}`. Take the **first** entry whose `to.name` equals `target_name` exactly. Never match on `name` or `id`: from `In Review`, `Done` is offered twice (research §4). None: `jira_refused` (`no_transition`).
5. **The estimate.** Only for `target` `done`: when some estimate field reads `null`, one `editJiraIssue {cloudId, issueIdOrKey, fields: {<id>: 1, …}}` writes 1 into each empty field, plus `assignee: {accountId}` when step 3a requires it (one call, never two). Absent keys and any other value, 0 included, are left alone. This replaces today's second `getJiraIssue` (`pi/lib/jira-transition.ts:88`), because step 2 already read the fields.
6. **The move.** `transitionJiraIssue {cloudId, issueIdOrKey, transition: {id: "<id as a string>"}}`. It must answer `isError` false with `{"success": true, "statusName": …}`; anything else is `jira_failed`.

Calls per run: one `getJiraIssue`, always. `getAccessibleAtlassianResources` and `atlassianUserInfo` only on a cache miss (the same row, so normally together). `getTransitionsForJiraIssue` and `transitionJiraIssue` only when behind. `editJiraIssue` at most once per run: when the assignee differs, or (target `done`) an estimate is empty; it is the only call an `unchanged` run can make.

### 3.3 Answer (stdout)

Success, a move:

```json
{"ok": true, "outcome": "moved", "ticket": "EEC-1", "target": "in_review", "cloud_id": "b4c1d0e2-…", "account_id": "712020:5b1c…", "from": "In Progress", "to": "In Review", "summary": "Retry failed payouts", "estimates": {}}
```

Success, already at or past the target:

```json
{"ok": true, "outcome": "unchanged", "ticket": "EEC-1", "target": "in_progress", "cloud_id": "b4c1d0e2-…", "account_id": "712020:5b1c…", "status_name": "In Review", "status_category": "indeterminate", "why": "past_target", "summary": "Retry failed payouts", "estimates": {}}
```

`account_id` is required in every success line, the one used (sent or resolved); a line without it is `jira_helper_failed` at Rust (pro-d454). `estimates` holds the values read in step 2, with each value this run wrote shown as `1`.

Failure: `{"ok": false, "error": {"code", "message", "details"}}`.

| Code | Exit | When | `details` |
|---|---|---|---|
| `usage` | 2 | stdin is not §3.1 | none. Rust maps it to `jira_helper_failed` at the engine |
| `jira_auth` | 3 | sign-in missing: `needs-auth`, the markers, 401 or 403 on any call, or no site with both scopes | `reason` when known |
| `jira_refused` | 1 | `site_ambiguous`, `issue_not_found`, `no_transition` | `reason`, always |
| `jira_failed` | 1 | a failed read, edit or transition for a reason a retry may cure (429, 5xx, 400 on the transition, text that is not JSON, any other tool error) | `tool`, `status_code` when known |
| `jira_helper_failed` | 1 | pi's modules did not load or are of another shape | `message` |

`spawn_failed` and `tool_timeout` are raised by `core/src/jira.rs`, not by the helper. A JSON line that is not one of these is `spawn_failed`.

### 3.4 The Rust seam

`core/src/jira.rs` keeps `call` and `answer`. `Request` gains `cloud_id`, `account_id: Option<String>` beside it (`None` serializes `null`: resolve it; pro-d454), `target_name`, `later_names`, and `estimate_fields` replaces `names`. `Done` becomes an answer that carries the facts of §3.3, `account_id` included, required. `HELPER_CODES` is unchanged.

## 4. The rank table

| Target | Rank | Policy key | Later targets (for `later_names`) |
|---|---|---|---|
| `in_progress` | 1 | `pr.jira.in_progress` | `in_review`, `done` |
| `in_review` | 2 | `pr.jira.in_review` | `done` |
| `done` | 3 | `pr.jira.done` | none |

**Forward coalescing** ([swr.md §8](swr.md), `Coalesce::Forward(rank)`). Let `r` be the rank of a new transition of key `K`:

- If a **`pending`** transition of `K` has rank **≥ `r`**, the new one is inserted `superseded` at once and nothing else changes.
- Otherwise the new one is inserted `pending`, and every `pending` transition of `K` with rank `< r` becomes `superseded`.

The `≥` is what [swr.md §8](swr.md) now says. Its earlier `higher` left equal ranks to duplicate, and the Job's rule "a later target supersedes unstarted earlier ones" needs equal ranks to collapse too.

An `in_flight` transition is never superseded. A `revalidating` one does not block. Only `pending` rows count.

## 5. The skip rule

A transition is **behind** when none of the three tests of §3.2 step 3 holds: its status is not at the target, not in the done category, and not at a later target. A transition that is not behind is **skipped**: its mutation becomes `done` through `skip` (§8) with **no Jira write**.

The rule is applied twice:

- **Before the helper runs**, to the cached row (`swr_jira_issues`). When the cached row exists and is not behind, the mutation is skipped and no process starts. A missing row, or a cached status that is behind, runs the helper.
- **On the helper's fresh read** (§3.2 step 3). An `unchanged` answer is a skip, with the fresh read committed to the cache.

The cached test can skip on a status Jira has since left. That is accepted: the rule is forward-only, and a run that is spawned always decides on a fresh read. Its `later_names` come from the policy read in step 3 of §8, the same read as the target name.

A `cancelled` Workflow's transition is skipped the same way, with no spawn. A target whose policy name is empty is skipped too (the policy names no status: no transition, as today).

## 6. The overlay

The effective state of a `jira:issue:<TICKET>` row is the cached `swr_jira_issues` row with `patch_json` applied, in `submitted_at` order ([swr.md §9](swr.md)). The Jira merge sets two fields only:

```text
apply(raw, patches): for each patch in submitted order:
    raw.status_name     = patch.status_name
    raw.status_category = patch.status_category
```

- **J2. Where the patch's name comes from.** The enqueue site reads the Workflow's policy before it opens its transaction, and takes `pr.jira.<target>` as `status_name`. This is the one policy read the enqueue sites gain. It is needed because the overlay must show a name, and a name exists only in policy. If the policy does not resolve (`owner_unset`, `policy_invalid`, and the rest of §8.3), the row is enqueued with `patch_json` NULL and the step still succeeds. That is deliberate: a Jira overlay must never block recording a merge (hs-d261). The row's execute resolves the policy again and reports the failure then.
- A target whose name is empty gets `patch_json` NULL (no overlay).
- **J3. The category.** `status_category` is `done` for target `done`, and `indeterminate` otherwise. These are Jira's standard category keys. The next read of the ticket overwrites them.
- The overlay lasts while the mutation is `pending`, `in_flight` or `revalidating`. It is dropped at once by `fail` (`failed`) and by `skip` (`done`). A `pending` retry keeps the overlay: a transition that is retrying is still optimistic.

## 7. Enqueue

The enqueue sites stay where they are, and one is added (`pr::open::record`, `in_review`; pro-d454). Each one's `jira_sync::queue(tx, plan, key, target)` call becomes an `enqueue` of the op of §2 with `Coalesce::Forward(rank)`, in the same transaction as today:

| Site (this branch) | Target | Written when |
|---|---|---|
| `core/src/driver/start.rs:680` | `in_progress` | an Attempt starts |
| `core/src/pr/post.rs:198` | `in_review` | `pr_posts` is inserted new |
| `engine/src/watch/team.rs:305` | `in_review` | the team review queues a post |
| `core/src/pr/poll.rs:974` | `done` | a merge is recorded |
| `core/src/pr/open.rs` `record` | `in_review` | the Workflow binds an open GitHub pull request, whether `cpd pr open` made it or `cpd workflow add --adopt` adopted it (both become `open` through `pr::open::record`; there is no `pr adopt`). It calls `crate::swr::source::jira::enqueue_for_workflow(tx, plan, workflow, 'in_review')` (pro-d454) |
| `core/src/pr/open.rs:541` | `done` | an adopted pull request is already merged |
| `adopt_ticket` (`core/src/swr/source/jira.rs`) | the target of the Workflow's status | a ticket is set on a Workflow (below) |

When the pull request is already merged, `record` enqueues `in_review` and then `done` in the same transaction; the Forward rule (§4) supersedes the `in_review` row, so only `done` runs.

`context_json` carries `plan` and `workflow`, which the event scope and the reopen need. An enqueue writes nothing else: it never calls Jira and never calls the helper.

**Ticketless Workflows.** When the Workflow's `jira` is NULL, the row is enqueued with `target_key` NULL, which `claim_next` never takes (§8). When `workflow add --jira` or `workflow edit --jira` sets the ticket, the same transaction enqueues each of the Workflow's ticketless `pending` rows again under `jira:issue:<KEY>`, in rank order, with the same Forward rule, and sets the ticketless rows `superseded`. This keeps today's behaviour, where rows of a Workflow with no ticket wait and run once it has one ([commands.md §21.11](../commands.md)).

**`adopt_ticket`** (pro-d454). Setting a ticket on a Workflow (`workflow add --jira`, `workflow edit --jira`) calls `adopt_ticket`, which, besides the re-enqueue above, also enqueues the target of the Workflow's current status under `jira:issue:<KEY>`: `merged` → `done`, `open` → `in_review`, `dispatch` with a started Job → `in_progress`. `waiting` (and `cancelled`) map to no target and enqueue nothing. A target already covered is not enqueued again: a `pending` or `in_flight` row of the key with rank ≥ the target's, or one the ticketless re-enqueue just wrote, covers it (the Forward rule of §4 would supersede it anyway). A Workflow made by `workflow add` is always `waiting`, so only `workflow edit --jira` reaches a non-empty target in practice.

**J6. Command outputs do not change.** The enqueues above are side effects of their steps and commands, not the commands' own writes. A6's `{queued, mutation_id}` applies to a command's own external write (`cpd pr post` posts to Slack and returns its own output). The Jira transition is queued, and its output is not added to the command.

## 8. Running a transition (the `jira` source in `swr`)

Today's code, which this replaces (origin/main): `engine/src/watch/jira.rs` `run` (115-182), `collect` (185-263), `fail_row` (86-113) and `start` (302-337); `core/src/pr/jira_sync.rs` `queue` (106-118) and `refuse` (145-181).

`swr` ([swr.md §12](swr.md)) runs the `jira` source. For each claimed mutation:

1. `claim_next(system = jira)` takes the oldest `pending` row whose `target_key` is not NULL and whose `next_attempt_at` is due.
2. The Workflow of `context_json` is looked up. If it is `cancelled` or gone: `skip`.
3. The target name and `later_names` are read from the policy now, when the row runs, as `jira-transitions` reads them today. An empty target name: `skip`.
4. The cache test of §5. Behind or no row: continue.
5. The helper runs, detached (see below), with the request of §3.1.
6. The answer maps as the table below.

**Detached.** The helper runs on its own thread, one run at a time for the `jira` system, and its answer is collected at a later tick. This is `jira-transitions`'s rule, kept: an engine tick never waits on Atlassian. `ExecOutcome` (§11 of swr.md) is delivered on the tick that collects the answer.

| Answer | Outbox | Cache and endpoint | Event |
|---|---|---|---|
| `ok` `moved` | one transaction: `complete` (`revalidating`, `completed_at` = now) and `cache::commit` of the facts with `poll_started_at` = now. The commit settles the mutation (§10, J5) | `swr_jira_issues` written by the commit (Changed). `swr_atlassian` written when the run resolved the cloud id or the account id (`cloud_id`, `account_id`, `fetched_at`). `record_ok('jira')` | none |
| `ok` `unchanged` | `skip` (`done`) | the facts committed with `poll_started_at` = the time before the read. `record_ok('jira')` | none |
| `jira_refused`, any reason | `fail` with `Retry::Never`, `error` = `jira_refused: <reason>`, and `context_json.refused_with` = `<ticket>\|<status name>` | `record_ok('jira')`: the endpoint answered | one plan-scoped `error` Event `jira_refused`, in the same transaction |
| `jira_auth`, `jira_not_configured` | `fail` with `Retry::After(watch.error_repeat_seconds)` | `record_failure(Permanent)`: due at once, once per outage | one machine-wide `error` Event (the existing code, no Plan) |
| `jira_failed`, `jira_helper_failed`, `spawn_failed`, `tool_timeout` | `fail` with `Retry::After(watch.error_repeat_seconds)` | `record_failure(Transient)` | one machine-wide `error` Event after 10 min of outage (A3, §9) |
| a store error settling the row | `fail` with `Retry::After(...)` | not an endpoint failure | its own code, as today |

The `jira_refused` row is parked: it is `failed`, and §9 decides when it comes back. Two more rules:

- **The cloud id.** A run that sent a cached `cloud_id` and failed with `jira_refused` `issue_not_found` or with `jira_failed` clears `swr_atlassian.cloud_id` and `swr_atlassian.account_id` together. The next run resolves the site and the user again. A failed assignee edit (`jira_failed`) is such a failure. The reason is that a cached id may name a site the account no longer reaches.
- **Scope of the outage Event.** A transient failure belongs to the endpoint, not to a Plan, so its `error` Event has no `plan`. It names the endpoint `jira`, the code, the message, and the instant the outage began. This replaces the per-row Event of today's R1 to R3 (commands §21.11). That is the change A3 asks for. The Event's `details` carry `endpoint` `jira` and `since`.

## 9. Parked transitions and the reopen

`refused_with` lives in `context_json`. Each `swr` pass, for each `failed` transition that has `refused_with`, oldest first:

1. Read the Workflow's current `jira` and the policy name for the row's target.
2. If the name is non-empty and `<ticket>|<name>` differs from `refused_with`, and no `pending` or `in_flight` transition of the new `target_key` has rank ≥ the row's rank, enqueue the same op again under the current ticket.

This is the old `jira-transitions` reopen, as an enqueue. Step 2's guard makes it idempotent: the new row is `pending`, so the next pass finds it and enqueues nothing. A refusal that repeats with the same key is not reopened.

The `failed` row stays as it is. It is a record, not a state that comes back.

## 10. Reads, freshness and the fence

**Reads.** A reader reads the effective state (§6): the `swr_jira_issues` row plus overlay. No reader calls Jira.

**No loop.** `due` returns nothing for `jira`. `fetch` makes no call. A `jira:issue` row is never stale: its freshness is what the last transition left, and no staleTime applies (add to [swr.md §4](swr.md): *Jira issue: never stale, no read loop*). `invalidated` on a `jira:issue` key waits for the next transition run of that ticket, since the source has no fetch.

**J5. The fence for a Jira write.** [swr.md §10](swr.md) requires a poll to record `poll_started_at` **before** it reads, and to settle a `revalidating` mutation only with a fetch that started after `completed_at`. Jira has no poll, so a read started after the write would never come. The transition's own answer is the read that does: `transitionJiraIssue` reports the status after the write. So for a `moved` answer, `complete` and the commit run in one transaction with `completed_at` = `poll_started_at` = the instant the answer arrived, and the commit settles the mutation. This is the one exception to §10's "before" rule. It is reversible, and §13 asks the Lead to confirm it.

The read before a transition is committed with `poll_started_at` = the time before that read. It cannot settle anything, because the mutation that runs is `in_flight` and not yet `revalidating`.

**gc.** A `swr_jira_issues` row's `inactive_at` is set by `swr`'s `jira` source when no Workflow with that ticket is `waiting`, `dispatch` or `open` and no `pending`, `in_flight` or `revalidating` transition has its key. It is cleared (`touch`) when one is. `gc::sweep` deletes the row after `swr.gc_days` ([swr.md §4](swr.md)). `swr_atlassian` is a single row with no `inactive_at` and is never swept.

## 11. What is retired

Each item goes in the same PR as the cutover ([swr.md §16](swr.md): cutover is one PR, no dual write).

| Retired | Where | Replaced by |
|---|---|---|
| table `jira_transitions` | `core/src/store/schema.rs`, its DDL in [contract §4.4](../contract.md) | `swr_mutations` rows of system `jira` (§2), dropped by §12's migration |
| `jira_sync` queue module: `queue`, `next_due`, `mark_settled`, `refuse`, `parked`, `reopen`, `Due`, `Parked`, `Refusal`, `Outcome` | `core/src/pr/jira_sync.rs` | `enqueue` (§7), `claim_next`, `skip`, `fail` (§8), §9 |
| `Target` enum | `core/src/pr/jira_sync.rs`, used by `core/src/jira.rs` and `engine/src/watch/report.rs` | the rank table of §4, in `core/src/swr/source/jira.rs` |
| engine step `jira-transitions`: `run`, `collect`, `fail_row`, `reopen_parked`, `start`, `State` | `engine/src/watch/jira.rs` and `engine.jira` | the `jira` source of `swr` (§8) |
| per-row Report key (`jira-transitions`, Plan, Workflow, target) and the per-row failure Events R1 to R3 | `engine/src/watch/report.rs`, [commands §21.11](../commands.md) | endpoint outage Events (§8, A3) |
| the helper's decision logic reading `names` for all three targets, and its second `getJiraIssue` | `pi/lib/jira-transition.ts` | §3 |
| the `names`-based request | `core/src/jira.rs` (`Request`, `stdin`) | §3.1 |
| plan-delete of `jira_transitions` rows | `core/src/plan/delete.rs`, [contract §4.14](../contract.md) | a delete of the Plan's `swr_mutations` rows of system `jira`, `json_extract(context_json, '$.plan')` |
| the tests of those paths | `cli/tests/engine/watch_jira.rs`, `cli/tests/flow/driver_jira_queue.rs`, `cli/tests/flow/plan_delete.rs`, `cli/tests/workflow/workflow_jira_queue.rs`, `core/src/store/schema/jira_transitions_tests.rs`, and the `jira_transitions` line of `core/src/store/schema/tests.rs` | the tests of §14 |

Kept: the `error` Event codes `jira_refused`, `jira_auth`, `jira_not_configured`, `jira_failed` and `jira_helper_failed` (the protocol is unchanged, A3). The `[pr.jira]` policy keys, their defaults and their validation (contract §8.3). The migration history of `20261102T1500-jira-transitions` (it stays in the history: migrations are never edited, contract §4.5).

## 12. The migration

Two migrations, one per owner:

- **swr-core's additive migration** (A1) creates `swr_jira_issues` and `swr_atlassian`. This page does not name it.
- **This Workflow's breaking migration**, `<UTC minute>-jira-swr`, made with `scripts/new-migration.sh jira-swr breaking`. It copies the **open** rows of `jira_transitions` into `swr_mutations` and then drops the table, which is breaking (A1). Its header is `-- cpd-migration: breaking after <n>`, with `n` counted by the code Job.

```sql
-- Every open row (PENDING, or PARKED as refused) of a Workflow with a ticket.
INSERT INTO swr_mutations (system, target_key, op_kind, vars_json, patch_json, status, submitted_at, context_json, error)
SELECT 'jira',
       'jira:issue:' || p.jira,
       'transition',
       json_object('ticket', p.jira, 'target', t.target,
                   'rank', CASE t.target WHEN 'in_progress' THEN 1 WHEN 'in_review' THEN 2 ELSE 3 END),
       NULL,                                   -- no overlay back-fill
       CASE WHEN t.refused_at IS NULL THEN 'pending' ELSE 'failed' END,
       t.queued_at,
       CASE WHEN t.refused_at IS NULL
            THEN json_object('plan', t.plan, 'workflow', t.pr)
            ELSE json_object('plan', t.plan, 'workflow', t.pr, 'refused_with', t.refused_with) END,
       t.refusal
FROM jira_transitions t JOIN workflows p ON p.plan = t.plan AND p.key = t.pr
WHERE t.settled_at IS NULL AND p.jira IS NOT NULL;

-- Open rows of a Workflow with no ticket: they wait, as today (§7).
INSERT INTO swr_mutations (system, target_key, op_kind, vars_json, status, submitted_at, context_json)
SELECT 'jira', NULL, 'transition',
       json_object('target', t.target,
                   'rank', CASE t.target WHEN 'in_progress' THEN 1 WHEN 'in_review' THEN 2 ELSE 3 END),
       'pending', t.queued_at, json_object('plan', t.plan, 'workflow', t.pr)
FROM jira_transitions t JOIN workflows p ON p.plan = t.plan AND p.key = t.pr
WHERE t.settled_at IS NULL AND t.refused_at IS NULL AND p.jira IS NULL;

-- Forward rule, once: a pending row with a lower rank than another pending row of its key is superseded.
UPDATE swr_mutations SET status = 'superseded'
WHERE system = 'jira' AND status = 'pending' AND target_key IS NOT NULL
  AND json_extract(vars_json, '$.rank') < (SELECT MAX(json_extract(b.vars_json, '$.rank')) FROM swr_mutations b
        WHERE b.system = 'jira' AND b.status = 'pending' AND b.target_key = swr_mutations.target_key);

DROP TABLE jira_transitions;
```

Rows that settled (`moved`, `unchanged`, `skipped`) are not copied: a finished move has no work left. A ticketless `pending` row is copied with `target_key` NULL, which `claim_next` never takes, so it waits for its ticket (§7).

## 13. Decisions this page makes

Each is one line and cheap to undo, except J5, which is still a choice about the fence.

- **J1.** Target key `jira:issue:<TICKET>`, the query key, not the Job's `jira:<TICKET>`.
- **J2.** The enqueue sites read the policy name for the overlay, before their transaction. A policy that does not resolve enqueues without an overlay and does not fail the step (hs-d261). This is the one new policy read on the enqueue path.
- **J3.** Overlay category `done` or `indeterminate`, by target, until the next read.
- **J4.** The helper keeps the fresh-read skip comparison, on `target_name` and `later_names` that Rust passes it, so it holds no rank table. A7 asks the helper to do I/O only. The comparison is three string tests, and the Rust copy in §5 and the helper copy are pinned by tests (§14). The alternative is a second helper round trip per run, which costs one more MCP connection per move.
- **J5.** The fence exception of §10: a `moved` answer settles its own mutation, because the answer is the post-write read and Jira has no poll.
- **J6.** Side-effect enqueues do not change the commands' outputs (§7).
- **J7.** An outage Event is machine-wide, not per Plan, and comes after 10 minutes (A3). A transient Jira failure no longer raises a plan-scoped Event at once. That is a visible change for the Lead.

## 14. Tests the code Job writes

Each test uses `FakeRunner` for the helper, `ManualClock` for time, and no real process, network, sleep or tmux ([swr.md §16](swr.md)). Unit tests sit in `core/src/swr/source/jira_tests.rs`, declared in the parent module.

- **Forward.** `in_review` enqueued over a pending `in_progress` supersedes it. `in_progress` over a pending `in_review` is `superseded` at once. A repeat of a pending rank is `superseded` at once. An `in_flight` one is never superseded.
- **Skip.** A cached row at the target, in the done category, or at a later target produces `skip` and no spawn. The helper's fresh `unchanged` produces `skip` and a committed fact.
- **Settle.** A `moved` answer leaves no `revalidating` mutation behind after its transaction (J5). A pre-read commit settles nothing.
- **Refusal.** `jira_refused` parks the row as `failed`, with one plan-scoped Event. A changed `<ticket>|<name>` enqueues once. A repeat of the same key enqueues nothing.
- **Failure.** `jira_auth` gives one machine-wide Event at once. A transient failure gives no Event for 10 minutes, then one endpoint Event. A failure never overwrites or deletes the cached row ([swr.md §16](swr.md)).
- **Cloud id and account id.** A fresh `swr_atlassian` row is sent as `cloud_id` and `account_id`. A row older than 24 h is sent as `null` for both. A failed run that sent a cached id clears both on `jira_refused` `issue_not_found` or `jira_failed`.
- **Assignee.** The helper assigns the authenticated user whenever the assignee differs, also when the outcome is `unchanged`, in the one `editJiraIssue` call that writes the `done` estimates (pro-d454).
- **Helper.** The request and answer shapes of §3, with the fake connection and the helper's comparison on the same strings as the Rust test of §5.
- **Migration.** A store before the breaking migration, with open rows of both kinds, comes out with the rows of §12 and no `jira_transitions` table.

## 15. Open items

None block this page. Two are for the code Jobs:

- The exact `<n>` of the breaking migration's header and the `<UTC minute>` name (the code Job counts them, as `pr-restack` did).
- The order of the `jira` source against the other sources in `swr` ([swr.md §18](swr.md)).
