# Stale-while-revalidate: the Slack datasource (`slack`)

Status: **normative for the Slack SWR Workflow** (Plan `external-sync-arch`, Job `swr-slack-docs` writes this page; the Workflow that cuts Slack over builds it). Parent design: [swr.md](swr.md) (cache, outbox, overlay, fence, E24). Schema: [contract §4.4 "The SWR tables"](../contract.md). Decisions: pro-d444 (Slack), pro-d440, pro-d448 (all sources, one DAG), pro-d449 (cpd is the backend). Accepted assumptions of swr.md §17 apply, with A5 and A6 as stated here.

The Lead's answer to the one open question (swr-slack-docs-a1-q1) is in §6.2 and is the rule for E11.

## 1. What moves

| Today | After the Slack cutover |
|---|---|
| `run/team-review.json`, written by E11 (`engine/src/watch/team.rs`) and read by E8 and `inbox claim` | `swr_slack_posts` rows of the query `slack:channel:<C>` |
| E11 spawns the helper and writes the file, and confirms own posts (`team.rs` `confirm_own_posts`) | the `slack` Source (`core/src/swr/source/slack.rs`): `fetch` runs the helper, `apply` writes the rows and confirms posts |
| the `slack_reactions` queue and E17 (`core/src/pr/reactions.rs`, `engine/src/watch/react.rs`) | `swr_mutations`, `system` `slack`, `op_kind` `reaction` |
| `pr post` sends at once and writes its row after the send | `pr post` enqueues `op_kind` `post`; E24 sends it, then writes the row |
| `pr repost` sends at once | `pr repost` enqueues `op_kind` `repost` |
| `watch.team_review_seconds` (contract §8.2) sets the read cadence | the key stays and is the cadence of the channel query (default 1800 s, the 30 min staleTime of swr.md §4); a failed read is retried 60 s later (the Lead's decision, swr-slack-impl-a1-q4) |

The Slack helper `pi/lib/slack-channel.ts` stays and gains a stdin. The send helper `slack-post.ts` and the reaction helper `slack-react.ts` stay: they are the `execute` I/O (A7). Nothing else in Slack is cached or queued outside SWR (contract §4.7).

## 2. Query, endpoint and tables

- **Query key** `slack:channel:<C>` (one per `inbox.team_channel`). `system` `slack`. staleTime **30 min**. Its row holds the data hash of the channel answer (§3.4).
- **Endpoint** `slack` (swr.md §7). Failures are classified in §3.6.
- **Table** `swr_slack_posts`: one row per **(message, pull request)**, not per message. Amended DDL (replaces contract §4.4 `swr_slack_posts` and swr.md §3.9):

```sql
CREATE TABLE swr_slack_posts (
  channel        TEXT NOT NULL,                       -- Slack channel id, C…
  ts             TEXT NOT NULL,                       -- Slack's message ts, exact string (ts_raw)
  repo           TEXT NOT NULL,                       -- owner/name of a pull request the message links
  number         INTEGER NOT NULL,
  primary_pr     INTEGER NOT NULL CHECK (primary_pr IN (0, 1)),  -- 1 for the first PR URL of the message
  author_id      TEXT,                                -- the (U…) of the header, "" when none
  author         TEXT,                                -- the display name
  author_is_me   INTEGER NOT NULL CHECK (author_is_me IN (0, 1)),
  reply_count    INTEGER,
  latest_reply   TEXT,                                -- Slack ts of the latest reply
  reactions_json TEXT NOT NULL,                       -- {"<name>": <count>}, counts from the channel read
  claim_json     TEXT,                                -- the claim object of §3.2, NULL when none
  eyes_me        INTEGER CHECK (eyes_me IS NULL OR eyes_me IN (0, 1)),
  thread_tags_me INTEGER CHECK (thread_tags_me IS NULL OR thread_tags_me IN (0, 1)),
  fetched_at     TEXT NOT NULL,
  inactive_at    TEXT,                                -- set when the channel answer no longer holds the message (§3.5)
  PRIMARY KEY (channel, ts, repo, number)
) STRICT;
```

Why the key has `repo, number`: a message can link two pull requests, and the match rule of contract §2.2 ("PR post") counts a link anywhere in the text, not only the primary one. One key of `(channel, ts)` would drop the second. `reactions_json` and the thread fields are per message and repeated on each of its rows.

Column meanings that differ from swr.md §3.9: `primary_pr` (E8 reads primary rows only, as today's `primary`), `author_id` and `author` (kept for E11's match, contract §2.2 (a)), `reactions_json` now holds **counts** (today the helper drops them, `pi/lib/slack-parse.ts` `parseReactions`), `claim_json` (the claim object), `eyes_me` and `thread_tags_me` are NULL until read.

## 3. The helper: `slack-channel.ts`

### 3.1 Invocation

Argv is unchanged: `node <adapter>/lib/slack-channel.ts --coding-agent <…/dist/index.js> --channel <C…> --me <U…>`, with `PI_CODING_AGENT_DIR`, cwd `$CPD_HOME`. The new part is **stdin**: one JSON value. It is started through the Runner port, `cpd_core::slack::call(config, "slack-channel.ts", args, Some(stdin), DEADLINE)` (core/src/slack.rs:18), run with `Runner::run(&Call)`, which writes `call.stdin` and closes it (core/src/host/runner.rs:33). Today E11 calls `runner.output` with stdin null (team.rs:151), so the Source's call is the one place that changes. Deadline 120 s, as today.

### 3.2 Input (stdin), fields

| Field | Type | Meaning |
|---|---|---|
| `version` | `1` | the contract version; any other value is `usage` |
| `known` | array | one entry per `swr_slack_posts` row whose `inactive_at` is NULL (§2) |
| `known[].ts` | string | the message ts, exact |
| `known[].repo`, `known[].number` | string, integer | the pull request of the row |
| `known[].author_is_me` | boolean | the stored `author_is_me` |
| `known[].reply_count`, `known[].latest_reply` | integer or null, string or null | the stored thread values |
| `known[].reactions` | object | the stored `reactions_json` |
| `known[].eyes_me`, `known[].thread_tags_me` | boolean or null | the stored values; null when never read |
| `terminal` | array of `"owner/name#n"` | every pull request the rows name whose `pull_requests.state` is `merged` or `closed` (SQLite, read at the fetch) |

stdin is read in full; a malformed value or a missing `version` is `usage` (exit 2), nothing is read from Slack.

### 3.3 Output (stdout), fields

One JSON value, as today, now with the per-message state the store needs.

```json
{"ok": true, "channel": "C02JU6C2Z1D", "read": 74, "posts": [
  {"repo": "acme/pay", "number": 812, "primary": true,
   "ts": 1760951522.0113, "ts_raw": "1760951522.011300",
   "author": "Ryan Case", "author_id": "U039194B7D2", "author_is_me": false,
   "reactions": {"eyes": 2, "github-check-mark": 1},
   "reply_count": 3, "latest_reply": "1760953000.000100",
   "claim": {"kind": "claimed", "reason": "claimed with :eyes: in Slack"},
   "eyes_me": false, "thread_tags_me": false}]}
```

| Field | Source |
|---|---|
| `posts` | one entry per `(message, pull request)`, newest `ts` first, as today |
| `reactions` | counts from the channel read's `Reactions: name (n), …` line (the parser keeps the count, §3.7) |
| `reply_count`, `latest_reply` | the channel read's per-message thread summary, **unverified** (§7) |
| `eyes_me`, `thread_tags_me` | the read's answer when this pass read it; **the known value echoed from stdin** when the rule of §3.4 skipped it; `null` when neither |
| `author_is_me` | `author_id` equals `--me` |

A value the helper did not read and did not echo is never printed. The output is the complete state of every row for this pass, so cpd stores it as it is.

Errors keep today's envelope, `{"ok": false, "error": {code, message, details}}`, and today's exit codes (contract §3.7, commands §21.6). New: an invalid stdin is `usage`.

### 3.4 Skip rules

Per **message** (`ts`), because the thread and the reactions belong to the message. A message's thread read is skipped only when **every** pull request on it is skipped; each row takes the echoed values.

**Thread** (`slack_read_thread`):

| Rule | Condition, first match wins | Thread read | `thread_tags_me` |
|---|---|---|---|
| T1 | the message's author is the User (`author_is_me`) | skip | known |
| T2 | every pull request of the message is in `terminal` | skip | known |
| T3 | no `known` entry for the message | read | read |
| T4 | `reply_count` or `latest_reply` differs from the known values | read | read |
| T5 | `thread_tags_me` is not known (null) | read | read |
| T6 | otherwise | skip | known |

**Eyes** (`slack_get_reactions`). Eligible when the message has a `claim` and its `reactions` count of `eyes` is at least 1.

| Rule | Condition, first match wins | Eyes read | `eyes_me` |
|---|---|---|---|
| E1 | the message's author is the User, or every pull request of it is in `terminal` | skip | known |
| E2 | not eligible (no claim, or no `eyes`) | skip | null |
| E3 | eligible, and the known `reactions.eyes` count differs from the current count, or `eyes_me` is not known | read | read |
| E4 | otherwise | skip | known |

The rules are the Lead's (swr-slack-docs-a1 brief) plus two stated by this page, which the brief did not name: T5 and E2. T5 keeps a message whose thread was never read from staying unknown after its counts stop changing. E2 makes "no eyes" a definite `null` with no call.

**Known limit** (stated, not fixed): a swap of one `:eyes:` reactor for another keeps the count, so E4 keeps the old `eyes_me` until the count moves. Its cost is one stale claim owner until the next change.

**Known limit**: a message the User wrote (T1, E1) keeps whatever `thread_tags_me` it had, and it is `null` if none. E8 reads a `null` as "not tagged".

### 3.5 Commit, apply, and the rows

- **Hash.** `fetch` computes the data hash over the canonical JSON of `posts` (sorted by `ts` descending, then `repo`, then `number`, keys sorted), without `read`. Equal to `swr_queries.data_hash` is **Unchanged**: no write (D5).
- **Changed.** `cache::commit` runs `apply` in one `Store::write`: upsert every `posts` entry on its key, `fetched_at` = now, `inactive_at` NULL; set `inactive_at` = now on every row of the channel whose key is not in the answer (the answer is the newest 200 messages and the 7-day window, so a message that leaves it is inactive, as a rewritten file dropped it). Rows are never deleted here: gc deletes them after `swr.gc_days` (swr.md §4).
- **Confirm.** The E11 confirmation of §6 runs after **every** successful fetch, Changed or Unchanged. It writes only when it records a post or enqueues an `auto_merge`, which is a change of `pr_posts` or of the outbox, not of the channel (so D5 holds for the channel data).
- **Failure.** A failed helper run calls `cache::fail` and `endpoint::record_*` (§3.6) and writes no row (swr.md §16).

### 3.6 Endpoint classes

| Failure code | `Kind` | Why |
|---|---|---|
| `slack_failed`, `slack_unparseable`, `tool_timeout` | Transient | a Slack or network fault that heals (today's rule) |
| `slack_auth`, `slack_not_configured`, `slack_helper_failed`, `pi_not_found`, `spawn_failed` | Permanent | only the User or an install fixes these: the report is due at once (swr.md §7) |
| `slack_refused` on one reaction | not an endpoint failure | a refusal of one message, parked on its mutation (§4.2) |

The outage Event is the existing `error` Event, once per outage (A3, swr.md §7).

## 4. Outbox operations

### 4.1 The rule for `target_key` (a correction, stated)

The brief gives `target_key` as `slack:<channel>:<ts>`. This page uses **`slack:channel:<C>` for every Slack mutation**, and carries the message identity in `vars_json` and in the patch. The reason is in swr.md §6 and §10: `complete` invalidates `target_key`, and `settle` runs only when a **fetch of that key** commits. A key with no `swr_queries` row is never fetched, so a `revalidating` mutation on `slack:<C>:<ts>` would never settle and its overlay would stay. The channel query is the key that the fetch of these writes commits, so it is the key that settles them.

### 4.2 Operations

Canonical `vars_json`: keys sorted, no spaces (swr.md §8 `Dedupe`). Active key `(slack, slack:channel:<C>, op_kind)`.

| `op_kind` | `vars_json` | Coalesce | Patch (the overlay on `swr_slack_posts`, `pr_posts`) | `execute` (the helper, through `Runner`) |
|---|---|---|---|---|
| `reaction` | `{"channel","emoji","ts"}`, and `plan`, `workflow` for a merge reaction | `Dedupe` on `(channel, ts, emoji)`: an active mutation with the same `vars_json` is returned, nothing written | the message's `reactions` +1 for `emoji`; `eyes_me` = 1 when `emoji` is `eyes` (the User's own claim) | `slack-react.ts --channel --ts --emoji` (§4.3) |
| `post` | `{"channel","number","plan","repo","text","workflow"}` (`text` rendered at enqueue) | **none**: a post is never superseded or deduped. The one guard is `pr post`'s `already_posted` (§5.1), checked in the enqueue's own transaction | `pr_posts` of the Workflow: state `pending`, no `ts` yet | `slack-post.ts --channel` with `text` on stdin (§21.10) |
| `repost` | `{"channel","plan","text","thread_ts","workflow"}` | **none**: each `pr repost` is one reminder (commands §9.8b2) | none | `slack-post.ts --channel --thread-ts --reply-broadcast true`, stdin `Bibbidi-bobbidi-bump!` |

Reactions in use today, all `reaction` ops:

| Emoji | Caller today (grep of `core/`, `engine/`, `pi/`) |
|---|---|
| `eyes` | the claim, `core/src/inbox/cmd.rs:680` |
| `github-check-mark`, `github-changes-requested` | the review verdict, `core/src/pr/review.rs:112` and `:191` |
| the policy's `post.merged_reaction` | the merge, resolved when placed (contract §3.7 A1), `core/src/pr/poll.rs:973`, `core/src/pr/open.rs:540` |

`:memo:` has **no caller** anywhere in the code or docs today. The brief names it; this page keeps it as an allowed emoji name with no producer, and flags it (§8).

### 4.3 Reaction outcomes

`execute` maps the helper's answer to the outcome of swr.md §8:

| Helper answer | Outcome | Store |
|---|---|---|
| `ok` (`already` true or false) | `Done` → `complete` | the channel query is invalidated; the next fetch shows the new count and settles it |
| `slack_refused` | `Failed` → `fail(Retry::Never)` | the row is `failed` with `error` = the Slack error name; one `error` Event `slack_refused`, scoped to the Plan for a merge reaction, as today (contract §3.7 E17, A4) |
| any other code (§3.6) | `Retry { after: 600 s }` | the endpoint's failure is recorded; the outage Event is the endpoint's (A3) |

**Merge reactions and the parked policy.** A merge reaction's emoji is read from the Workflow's policy when it is placed (contract §3.7 A1), so the emoji is resolved at `execute`, not at enqueue. A parked merge reaction (`failed`) is **re-enqueued** by the Source's `due` on a pass where the policy names a non-empty emoji different from the parked row's (a new mutation, with the new `vars_json`). That replaces today's reopen-in-place (`react.rs` `reopen_parked`), because a `failed` mutation never revalidates (swr.md §8). A policy that names no emoji parks it with `policy_empty` and no re-enqueue.

### 4.4 Overlay and dedupe, in one place

- `reaction`: the key is `(channel, ts, emoji)`; two `reaction` mutations with that key share one active row, so a second click returns the first id (`queued` true, the same `mutation_id`).
- The overlay of a `reaction` adds its emoji to the message's counts on every row of the message. A `failed` or `superseded` one contributes nothing (swr.md §9).

## 5. The writes that enqueue

### 5.1 `pr post` and `pr repost` (the CLI)

`pr post` runs its checks 1 to 7 as today (commands §9.8b). Check 8 changes: the merge methods and `autoMergeAllowed` come from `swr_repos` (the repository row, 24 h staleTime), **not from `gh`**, because an external read outside SWR is refused (contract §4.7). Then, in **one** write transaction:

1. `already_posted` (exit 3) when a `pr_posts` row exists **or** an active `post` mutation exists for the Workflow (`details` `{workflow, mutation_id}` for the second). This is the one guard of §4.2.
2. insert the `post` mutation with its `vars_json`, and the Workflow's `pr_posts` overlay.
3. return `{ok, plan, workflow, queued: true, mutation_id, message}`. The `pr_posts` row does not exist until the send answers.

When E24 completes the send, `execute` writes the `pr_posts` row (`channel`, `ts`, `link`, `posted_at`, `last_posted_at`, `reaction` = the policy's `merged_reaction` when not empty, `auto_merge` `pending`), and the `in_review` Jira row when that insert is new, in the **same** transaction as the row (commands §9.8b Effects). Then it enqueues the `auto_merge` mutation (§6.3); the repo gate of §6.2 is in its executor.

`pr repost` (commands §9.8b2) checks 1 and 2 as today and enqueues `repost`. Its `last_posted_at` is set on completion, as today.

### 5.2 `inbox claim`

`inbox claim` (commands §21.7) enqueues the `eyes` `reaction` in its own transaction (`core/src/inbox/cmd.rs:680` today). Its precondition "the team review cache holds a primary post" (`post_not_found`) reads `swr_slack_posts` (`primary_pr` = 1 and `inactive_at` NULL) instead of the file.

## 6. E11: the confirmation of own posts

### 6.1 Today

`confirm_own_posts` (`engine/src/watch/team.rs:230`) confirms, for each open Workflow with an open pull request, no `pr_posts` row, and `pr.post.enabled` true, the earliest message of the User (`author_id`) that carries the pull request. It inserts the row through `posts::insert` (`core/src/pr/posts.rs:49`), which writes `auto_merge` `pending` for **every** confirmed post. E18 then asks GitHub (`engine/src/watch/auto_merge.rs`).

Checked against the contract (§2.2 "PR post", the match rule), this code has three defects:

| # | Contract (contract §2.2 "PR post") | Code (`team.rs`) | Fix in the Slack Workflow |
|---|---|---|---|
| D1 | (c) the message was posted **at or after the pull request's `created_at`** | no `created_at` test (the filter at team.rs:259-268 has none) | add the test; `pull_requests.created_at` is GitHub's |
| D2 | `posted_at` and `last_posted_at` are **the message's time** (so the 24 h overdue clock starts at the post) | `posted_at: now` (team.rs:297), the poll's time | use the message's `ts` |
| D3 | `reaction` is the owner's `pr.post.merged_reaction` when not empty | `reaction: None` (team.rs:298), so no merge reaction is ever queued for a confirmed post (`poll.rs:973` queues only a row with a reaction) | set it from the policy |

D1 to D3 are defects found by reading the code, **unverified by a run**. The test Job pins each with a fixture.

### 6.2 The rule (the Lead's answer, swr-slack-docs-a1-q1)

A post confirmed from the channel by E11 (an own message of an open Workflow whose repo policy has `pr.post.enabled` true) **queues an `auto_merge` outbox mutation** (`op_kind` `auto_merge`, `target_key` `github:pr:<repo>#<n>`), as it gets `auto_merge` `pending` today. The decision is **policy only**: it is not gated on `swr_repos.auto_merge_allowed` at confirm time. The gate is in the executor: the `auto_merge` op reads `swr_repos` (24 h, no `gh` call) and finishes as `not_allowed` without calling GitHub when `auto_merge_allowed` is 0, as today's `not_allowed` answer (stored `none`).

Nothing is queued when:
- the message is not an own message of an open Workflow with a confirmed PR match (no row to confirm);
- the Workflow's pull request is not `open`;
- the policy's `pr.post.enabled` is false.

**Open point, the User's call, unchanged by this Plan:** whether a message sent by hand (not by `pr post`) should ever enable auto-merge. The rule above includes it, as the brief does.

### 6.3 The `auto_merge` mutation

`pr post` (after its send) and E11 both enqueue `auto_merge` through the same function. The executor is the github Source's (swr.md §14), so the `auto_merge` op and its `not_allowed` gate belong to the github Workflow; this Workflow only enqueues.

**Ordering.** The Slack cutover must not enqueue `auto_merge` before the github Workflow's `auto_merge` executor is live: a mutation with no executor stays `pending` forever. Until then, E11 and `pr post` keep writing `pr_posts.auto_merge` `pending`, which E18 runs as today (`engine/src/watch/auto_merge.rs`), and they do not do both. The github cutover retires that column and E18 together (swr.md §15).

## 7. Unverified

- **The MCP output of `slack_read_channel`** (not verified by a scout): whether each message carries `reply_count`, `latest_reply` and a reaction summary with counts. The test Job must say the following. A helper test pins a **recorded fixture** of the tool's text, no real Slack. The implementation Worker verifies the real output with `cpd mcp attach slack` before it relies on the fields, and **escalates with `cpd ask` if any field is absent**. If `reply_count` or `latest_reply` is absent, T3 to T6 cannot skip and the thread read runs for every message: that is a design change, not a fallback.
- The text of `Reactions: name (n)` is read from today's `slack-parse.ts` fixtures, not from a live message.

## 8. Open items

- `:memo:` has no producer (§4.2). A caller must name it or it is dropped from the brief.
- The swap limit of `eyes_me` (§3.4) and the `thread_tags_me` of the User's own messages (§3.4).
- The `pr_posts.auto_merge` column is retired by the github Workflow's cutover, not here (swr.md §15); this Workflow stops writing its own queue and keeps the column until then.
- The E8 memo token of contract §2.2 "The team review rule" reads `swr_slack_posts` in place of the file. Its rule is unchanged here; the cutover Job wires it.

## 9. Retired at the Slack cutover

Each is removed in the cutover PR, with its old reader (swr.md §15, no dual write, no flag).

| Retired | Where | Replaced by |
|---|---|---|
| the team review cache `run/team-review.json` and its `.tmp` (contract §1.4) | `engine/src/watch/team.rs` `cache_path`, `store` | `swr_slack_posts` |
| E11, the team review step; its number is **not reused** (as E23, contract §3.7) | `engine/src/watch/team.rs` (whole file) | the `slack` Source under E24 |
| the file's readers | `core/src/inbox/team.rs` `read_cache` and `Cache::primary_post`, read by `core/src/inbox/rules.rs` `review_requested` (rules.rs:662) and `core/src/inbox/cmd.rs` (line 564) | `swr_slack_posts` queries |
| the Slack reaction queue API | `core/src/pr/reactions.rs` `queue`, `queue_merge`, `next_due`, `mark_reacted`, `parked`, `set_emoji`, `reopen`, `refuse` | `swr_mutations` (§4) |
| E17, the reaction step; its number is **not reused** | `engine/src/watch/react.rs` (whole file) | the `slack` Source under E24 |
| the reaction callers | `core/src/pr/poll.rs:973`, `core/src/pr/open.rs:540`, `core/src/pr/review.rs:191`, `core/src/inbox/cmd.rs:680` | enqueue (§4, §5) |
| `policy_emoji` and `repo_sets_reaction` | `core/src/pr/reactions.rs` | moved into `source/slack.rs`, read at `execute` (§4.3) |
| the table `slack_reactions` | contract §4.4 (migrations `slack-reactions` and `20261102T1300-slack-reaction-refusal`) | dropped by the Slack breaking migration (A1) |
| the config key `watch.team_review_seconds` (contract §8.2) | kept: it is the channel query's cadence (§1), not retired | |

**The breaking migration** (A1: a source Workflow adds only a breaking migration that copies open rows and drops a table it retires; name chosen by the Workflow, `<YYYYMMDDTHHMM>-swr-slack`). It copies each `slack_reactions` row with `reacted_at` NULL and `refused_at` NULL to a `pending` `reaction` mutation (`submitted_at` = `queued_at`, `vars_json` as §4.2), a parked row (`refused_at` set) to a `failed` mutation with `error` = `refusal`, and drops the reacted rows. Then it drops `slack_reactions`. `swr_slack_posts` is not migrated: the first fetch after upgrade is due at once and fills it.

Docs changed by the cutover PR, not by this page: commands §21.5 (the cache), §21.6 (the output of §3.3), §21.9 (the queue, replaced by §4), contract §1.4 (the file's row), contract §3.7 E11 and E17 rows (replaced by E24 for Slack), contract §8.2 (`watch.team_review_seconds`).

## 10. For the test Job

The Spec of the test Job must carry these rules:

- Every test mocks the helper, `gh` and time (FakeRunner, ManualClock, `core/src/ports.rs`; swr.md §16). No real Slack, no real process, no sleep.
- Each row of the §3.4 tables has a test that fails first, for the right reason: T1 to T6, E1 to E4, and the echo of a known value.
- The helper's own test pins a recorded fixture (§7). The implementation Worker verifies the real output with `cpd mcp attach slack`, and escalates if a field is absent.
- Each of D1 to D3 has a test that fails on today's code.
- The `reaction` dedupe, the `post` guard (`already_posted` on an active mutation) and the `repost` non-coalescing have tests.
- A failed or paused fetch never writes or deletes a `swr_slack_posts` row (swr.md §16).
