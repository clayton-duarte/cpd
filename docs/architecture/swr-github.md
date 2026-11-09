# SWR: the GitHub datasource

Status: **normative for the `github` Workflow** (Plan `external-sync-arch`). It is the design page that [swr.md §14](swr.md) names. The generic rules (the two clocks, the status axes, the fence, the outbox lifecycle) are in [swr.md](swr.md) and are not restated here; this page says how GitHub fills them. Where merged code later differs, the code wins and this page is corrected. Vocabulary: [CONTEXT.md — Data](../../CONTEXT.md). Decisions: pro-d440, pro-d441 (GitHub), pro-d446 (`swr`), pro-d447 (check-level `excused_labels`), pro-d448, pro-d449. Accepted assumptions A1 to A9 are those of [swr.md §17](swr.md); the ones this page adds are in §13.

Scope: the GitHub **read** of every pull request, repository, deploy and the viewer; the GitHub **writes** of the outbox; and the readers of their result. It does not design Slack, Jira, models or the classifier (their own pages).

## 1. The idea in one pass

One engine pass (E5, every `pr_poll_seconds`) sends **one** GraphQL read document to GitHub. Its answer is committed into SQLite, key by key. Every reader afterwards reads SQLite through one function, `swr::github::effective_pr`. Every GitHub write is an outbox row, drained by the engine, and shows at once as an overlay on the row it changes.

Three rules follow, and §16 of [swr.md](swr.md) binds them:

- A reader never calls GitHub. `cpd pr view`, `cpd inbox claim`, `cpd pr merge`, `cpd pr post`, E8, the TUI and `cpd plan show` all read the cache.
- A failed pass never overwrites or deletes a row. A rate-limit pause or an outage is a row of `swr_endpoints`, not a file.
- A write is visible before GitHub has it, and it stops being an overlay only when a pass that started after the write proves GitHub has it (the fence, [swr.md §10](swr.md)).

**Containment (`base_contained`).** For each open pull request whose `base_ref` is its repository's default branch, the poll sets `pull_requests.base_contained` from the one poll read: the default branch name is the Light read's `repository.defaultBranchRef.name` and the pull-request head oid is its `headRefOid`, both of the same pass (not the head stored by an earlier pass). The check is one `gh api repos/{repo}/compare/{default-branch-head}...{pull-request-head}` read per such pull request, made in that pass after the document is read. It is an additional per-PR read, not a second poll document, and it adds no cadence (`pr_poll_seconds`, [clocks](clocks.md)). The authoritative field is the compare response's `status`: `ahead` or `identical` means the pull request head contains the current default-branch head; `behind` or `diverged` means it does not. `mergeStateStatus` is not a containment signal (`UNKNOWN` does not establish containment). Unknown or missing inputs, and a compare that fails or does not answer, leave that pull request's flag NULL (unknown): an unknown flag is never treated as contained or as silently ready, and a failed compare does not fail the pass. A stacked pull request (base differs from the default branch) is not subject to this rule and keeps NULL. The cached flag serves readiness only: at merge enqueue, the merge gate reads fresh and checks containment again ([commands §9.10](../commands.md)).

E5 sends one GraphQL read document per `pr_poll_seconds` cadence ([clocks](clocks.md)); the containment compare is the per-PR read above, not a reason to add another poll/document cadence.

## 2. Query keys and tables

| Key (`github:<kind>:<id>`) | Row | Staleness (swr.md §4) |
|---|---|---|
| `github:pr:<owner/repo>#<n>` | `pull_requests` (light header) and `swr_pr_detail` (heavy) | open: 5 s; merged or closed: never (`stale_after` NULL) |
| `github:repo:<owner/repo>` | `swr_repos` | 24 h |
| `github:viewer` | `swr_viewer` (`login`) | 24 h |
| `github:deploy:<owner/repo>#<run_id>` | `swr_deploys` | the pass's own cadence; a deploy has no staleTime of its own |
| `github:issue:<owner/repo>#<n>` | `gh_issues` | **unused** (§12, seam (g)) |

`pull_requests` is the raw cache (accepted assumption A2). The swr-core migration `20261106T1659-swr` added to it `head_oid` and `gh_updated_at`. This page's pass writes them: `head_oid` is `headRefOid` and `gh_updated_at` is `updatedAt` of the light header. The existing column `updated_at` keeps its pro-d68 meaning (the stamp of the last feedback read); the heavy trigger of §4 reads `swr_pr_detail.updated_at` instead, so the two never disagree about what they mean. Retiring `updated_at` is an open item (§14).

`pull_requests` keeps its history triggers (contract §4.4). Every Changed commit of its header therefore writes a history row, and an Unchanged pass writes none (D5). `swr_pr_detail` and the other `swr_*` tables carry no history.

## 3. The one document

### 3.1 The builder

One builder in `core/src/pr/gh.rs` produces the whole read document: `pass_document(plan: &PassPlan) -> String`. Its input, `PassPlan`, is computed from the stored rows before the call (§4.1). Its output is one query text, sent inline with `gh api graphql -f query=<text>` (the existing rule: `-f query=@file` sends the literal `@file`). The builder is a pure function; it has a unit test per shape (§11).

The document, with the aliases the builder emits (literal values shown as placeholders):

```graphql
query {
  viewer { login }
  rateLimit { remaining resetAt cost }

  # Open pull requests the User authored. first:100 is GitHub's page size for a search.
  authored: search(type: ISSUE, query: "is:pr is:open author:@me archived:false sort:created-asc", first: 100) {
    pageInfo { hasNextPage endCursor }
    nodes { ... on PullRequest { ...Light } }
  }
  # Open pull requests whose review the User is asked for.
  reviews: search(type: ISSUE, query: "is:pr is:open review-requested:@me archived:false sort:created-asc", first: 100) {
    pageInfo { hasNextPage endCursor }
    nodes { ... on PullRequest { ...Light } }
  }
  # Only when the last pass answered hasNextPage: the next page, with the cursor that pass stored. Not built until the cursor column is decided (§3.3, §14).
  authoredMore: search(type: ISSUE, query: "...the same query...", first: 100, after: "<stored endCursor>") { ... }

  # One alias per absent or first-seen pull request (§4.2). Its repository alias is the lookup.
  p0: repository(owner: "<o>", name: "<n>") { pullRequest(number: <n>) { ...Light ...Heavy viewerDidAuthor viewerLatestReviewRequest { id } } }

  # One alias per repository that is missing from swr_repos or older than 24 h (§4.3).
  m0: repository(owner: "<o>", name: "<n>") {
    defaultBranchRef { name } isArchived autoMergeAllowed squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed deleteBranchOnMerge
  }
}

fragment Light on PullRequest {
  number state isDraft mergeable mergeStateStatus reviewDecision
  baseRefName headRefName headRefOid updatedAt title url createdAt mergedAt
  author { login }
  repository { nameWithOwner }
  labels(first: 100) { nodes { name } }
  autoMergeRequest { enabledAt }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
  latestReviews(first: 20) { nodes { state author { __typename login } commit { oid } } }
  reviewThreads(first: 100) { nodes { id isResolved } }   # owned pull requests only (§3.2)
}

fragment Heavy on PullRequest {
  latestReviews(first: 20) { nodes { state author { __typename login } commit { oid } } }
  reviewThreads(first: 100) { nodes { id isResolved isOutdated path comments(first: 20) { nodes { id author { __typename login } body url createdAt } } } }
  comments(first: 50) { nodes { id author { __typename login } body url createdAt } }
  commits(last: 1) { nodes { commit { oid statusCheckRollup { state contexts(first: 100) { nodes {
    __typename
    ... on CheckRun { name conclusion status detailsUrl checkSuite { workflowRun { databaseId workflow { name } } } }
    ... on StatusContext { context state targetUrl }
  } } } } } }
}
```

Notes on the shape:

- **`labels`, `autoMergeRequest`, `latestReviews` and `reviewThreads` are light** here, though the spec's list put them elsewhere. Reason: `labels` and `autoMergeRequest` are header scalars that the readers need on every pass; `latestReviews` (pro-d453: it replaces `latestOpinionatedReviews`, and adds `commit { oid }`) is what the human-approval rule of contract §7.6 step 2 (pro-d158) reads, so `review_decision` cannot be computed without it on every pass, and it gives every polled pull request its reviewers' state and reviewed commit without a heavy read. The pass stores the `latestReviews` nodes, the label names and the author's login in `pull_requests.reviews_json`, `labels_json` and `author_login` (migration `pr-light-cache`); `reviewThreads(first: 100){ id isResolved }` is what the pro-d387 resolution check reads (contract §7.6 step 3a), and a thread resolved on GitHub moves no `updatedAt`, so without it on every pass a resolution would be invisible. The last one is selected only for pull requests a Plan owns (the builder knows them from `workflows`).
- **`comments(first: 50)` is heavy, and `reviewThreads` is `first: 100` in both fragments (pro-d457): `Light` and `Heavy` read the same 100 threads.** The `first` counts are this page's choice. Reason: they bound one pull request's node count; a thread or comment beyond them is not read in that pass. Unverified: that 50 comments and 100 threads cover the feedback of a real pull request. The first pass that finds a truncation (`pageInfo` absent from these lists, so the builder reads `totalCount` on the same selection) must report it as an open item.
- The check contexts (`... on CheckRun`, `... on StatusContext`) replace `gh run list --commit` and the `gh pr view --json statusCheckRollup` of `pr view`. `checkSuite.workflowRun.databaseId` is the id `pr rerun` needs (§9, `rerun`).
- Repository metadata (`m<i>`) is fetched only for a repository missing from `swr_repos` or whose `fetched_at` is older than 24 h.

### 3.2 Which selections a pass sends

| Selection | Sent for |
|---|---|
| `Light` | every pull request in the document |
| the `reviewThreads(first: 100)` part of `Light` | every pull request a Plan owns (a `workflows` row points to it) |
| `Heavy` | a pull request whose `updatedAt` differs from `swr_pr_detail.updated_at`, whose `headRefOid` differs from `swr_pr_detail.head_oid`, or that has no `swr_pr_detail` row |
| `viewerDidAuthor`, `viewerLatestReviewRequest` | an absent-pull-request alias (§4.2), which needs them to decide `authored` and `review_requested` |
| `m<i>` (repository metadata) | §3.1's rule |

The `Heavy` fragment is sent only when the poll has heavy pull requests (a non-empty `Heavy` set); a poll with none sends no `Heavy` selection (pro-d457).

The `Heavy` test compares with `swr_pr_detail`, and it is the "moved since `hydrated_at`" test of the spec: a row whose `updated_at` or `head_oid` differs from what the last hydrate stored is re-read, and a successful hydrate sets `hydrated_at`. A pull request whose detail did not move, costs no heavy fields.

**Cap (assumption, unverified).** The builder sends at most 25 `Heavy` selections in one document; the rest wait a pass, oldest `hydrated_at` first. The cap is this page's guess at keeping one document under GitHub's documented 500 000-node limit, which this page has not measured. The builder's unit test asserts the cap, and `rateLimit.cost` of each pass is stored (§5) so a real cost can replace the guess.

### 3.3 Paging inside the document

The search returns at most 100 pull requests (`first: 100`, oldest first; the limit is `SEARCH_QUERY` of `core/src/pr/gh.rs:831` today). The design sends a second page **in the same document**, with the cursor the previous pass stored, and only when that pass answered `pageInfo.hasNextPage` true (`authoredMore` above). Pages beyond the second are not read: §4.2 covers every tracked pull request that a search does not list, so the limit hides none of them.

**Blocked on a column (open item, §14).** The cursor is opaque text, and no `swr_*` table has a free text column for it: `swr_queries.data_hash` is the hash of confirmed data and `swr_queries.last_error` is the text of a failure. Until a column is decided, the pass sends the first page only, and §4.2 alone covers the rest. This is the one clause of the spec's paging rule that this page does not yet build.

### 3.4 Failure of the document

A document that fails, times out or parses badly is one failure of the endpoint `github` (§7). It writes nothing: no header, no detail, no flag, no deploy row, no rate record other than the failure itself. An answer with `errors` that carry `RATE_LIMITED` is a rate-limit pause (§7.2), not an error.

## 4. The pass

Each step is one function in `core/src/swr/source/github.rs`; the reader is `core/src/swr/github.rs` (§6). The registered source is the live `GithubSource`, not the file's stub `Github`: only what `registry()` names runs (swr.md §11). `now` and `poll_started_at` are arguments (swr.md §16).

1. **Plan the document.** From the stored rows, before the call, compute:
   - the `Heavy` set and the alias sets (§3.2, §4.2);
   - whether `authoredMore` is sent (§3.3);
   - the repositories needing `m<i>` (§3.1).
2. **Record `poll_started_at = now`**, before the call (swr.md §10).
3. **Call once.** Through the `Runner` port (§7.3), with the endpoint's deadline. Any failure is §3.4.
4. **Parse and check.** A reply that does not parse, or lacks a field the document asked for, is one failure (`gh_failed`, permanent unless it is cut off, §7.1). A pull request listed by a search that carries no `headRefOid` or `updatedAt` is malformed and fails the whole pass.
5. **Commit each key, in its own transaction** (`cache::commit`, swr.md §6), with the `data_hash` of the data the pass read for it:
   - a pull request: its header into `pull_requests` (`state`, `draft`, `mergeable`, `merge_state`, `review_decision` by the pro-d158 human rule, `checks` by the step 3b rule of contract §7.6, `base_ref`, `default_branch`, `archived`, `head_oid`, `gh_updated_at`, `title`, `url`, the flags); its detail into `swr_pr_detail` when `Heavy` was sent;
   - the Events that a change raises (contract §7.6 step 6: `pr-feedback` reasons, `pr-merged`, `pr-closed`), in the same transaction as that change;
   - the review-Lead wake of guarantee (b) (§10), in the same transaction;
   - a repository: `swr_repos`; the viewer: `swr_viewer`.
   An unchanged key opens no write transaction (D5). The status-only write of swr.md §6 (`error` back to `success`, a `revalidating` mutation settled to `done`) is the only write an unchanged key can make.
6. **Merged and closed pull requests.** A header that reads `MERGED` or `CLOSED` sets `stale_after` NULL for its key (§5). A header that reads `MERGED` also closes the pull request's open `pr-ready` Inbox item in the same transaction, whichever path stores the merge (the poll's write or the header write of this pass). So a merge seen on poll recovery never leaves a stale item for E8's next reconcile (contract §2.2 "ROW LIFE"; E8 remains the backstop, which closes an item whose pull request is no longer open). A `CLOSED` pull request is stored `state` `closed`; its Workflow stays `open` (contract §3.8, "A PR closed without a merge", and the "Ready, never a draft" rule 4), so the Workflow's own rules decide the rest.
7. **Record the rate limit** (§7.2) from `rateLimit`, and record the success of the endpoint (`record_ok`).
8. **Settle** the `revalidating` mutations of the keys this pass read, whose `completed_at <= poll_started_at` (swr.md §10). A settled mutation's overlay goes; a mutation whose key the pass did not read stays overlaid.
9. **Deploys** (§8), after the header writes, as the existing step 8 of contract §7.6.
10. **Drain the outbox** for `github`: `claim_next` and `execute` (§9), unless the endpoint is paused (§7.2).

A pass that starts while a previous one still runs does not happen: the endpoint's `fetch_status` is `fetching` for the call (swr.md §16).

**A reopen is not a second pass (rule 1 of the spec).** Today a pass that reopens a Workflow runs again: `core/src/pr/poll.rs:102-106` loops on `again`, which `rebind_reopened` sets (pro-d362). The github Workflow removes the loop. A Workflow reopened by a commit is owned from the next pass; that pass's document selects its heavy fields, because it has no `swr_pr_detail` row yet (§3.2), and reads its checks and feedback then. The cost is one pass of latency, which E5's cadence already has.

### 4.1 Which pull requests a pass reads

A pull request is read by the document when one of the following holds. The rule decides only the document; the header writes of §4 step 5 follow the answer.

- **Listed:** the pull request is in `authored` or `reviews` of this document (§3.1). Every open pull request the User authored or is asked to review is listed every pass, up to the first 100 of each search and the pages of §3.3.
- **Absent:** the row is a candidate for an alias (§4.2).

### 4.2 The absent-pull-request aliases

The document carries one alias `p<i>` per **candidate**. A candidate is a stored `pull_requests` row with `state` `open` and `archived` 0 that satisfies at least one of:

- **(a) owned:** a `workflows` row of a Plan in status `open` points to it, and no search of the **previous** pass listed it;
- **(b) authored:** `authored` is 1 and the previous pass's `authored` search did not list it;
- **(c) review requested:** `review_requested` is 1 and the previous pass's `reviews` search did not list it;
- **(d) Slack-posted (pro-d453):** an active row of `swr_slack_posts` (the team review channel; `repo` and `number` set, inside the 7-day window) points to it, and it has no row in `pull_requests` yet (first seen) **or** its row is `open` and not archived. A teammate's pull request posted without a review request is neither authored nor requested, so without this it would be read once and then go stale; it is read every pass until it merges or closes. It is an alias at most once per document, and not when it is in the `Heavy` set.

"The previous pass" is the document the stored rows came from: the rows are built from it, so the candidate rule is decided before the call, which is why this design can send the alias in the same document. A pull request that left the open list between passes is therefore confirmed by the alias of the **next** pass, not the current one.

The rule's effect, and the reason for it:

- **Open-only (rule 2 of the spec).** A row whose stored `state` is `merged` or `closed` is never a candidate, so no alias reads it. The non-open reads are exactly two: the one confirmation read of a locally open pull request that left the open list (its answer sets `state`), and the first-seen Slack pull request (d); a Slack-posted pull request whose row is merged or closed is not read again. A confirmation answer of `OPEN` clears (b) and (c), whose flags follow what the searches list (contract §7.6 step 5, S4); an owned pull request stays a candidate while it is open (contract §7.6 step 3).
- **Limit (rule 3 of the spec).** A tracked pull request beyond the first page of a search is not listed by it, so (a) to (c) cover it, in the document of the next pass. No tracked pull request is hidden by the limit.
- **Archived (rule 2).** A repository that GitHub reports archived sets `archived` 1 on its rows; an archived row is not a candidate and the search's `archived:false` never lists it.

An answer of `NOT_FOUND` for an alias (the pull request or repository is gone) is recorded as `fail` for that key (swr.md §6: data kept, `error` set), and the row keeps its last values.

### 4.3 Repository metadata

`swr_repos` is read for a repository of a listed or aliased pull request when the row is missing or `fetched_at` is older than 24 h, through `m<i>` (§3.1). A repository whose `isArchived` reads true sets `archived` 1 on its pull requests in the same transaction, which removes them from the candidates of §4.2.

## 5. The clocks of the GitHub keys

- A **Changed** commit of `github:pr:<key>` sets `stale_after` = `now` + 5 s (open) and `fetched_at` = `now`.
- A **merged or closed** key sets `stale_after` NULL: it is never stale, and it is read again only when a search lists it (it cannot, since the search is `is:open`) or when a tracked open pull request is confirmed (§4.2). A merged row that a later listing reports as not merged changes nothing (contract §7.6 step 5).
- The in-memory fresh-until instants (swr.md §4) live in the engine. After a restart every key is due once.
- `rateLimit.remaining` and `resetAt` of each pass are stored as `swr_endpoints.rate_remaining` and `rate_reset_at` (§7.2). `rateLimit.cost` is not stored: the pass writes it to `watch.log`, one line per pass, which is enough to tune the §3.2 cap. Storing it is an open item (§14).

## 6. The reader: `swr::github::effective_pr`

```text
swr::github::effective_pr(store: &Store, repo: &str, number: i64) -> Result<Option<EffectivePr>, CpdError>
```

`core/src/swr/github.rs`, a new file of this Workflow. It reads, in one read transaction:

1. the raw `pull_requests` row of `(repo, number)`, or `None` when there is none;
2. the `swr_pr_detail` row of it, or `None` before the first hydrate;
3. the active mutations of the key `github:pr:<repo>#<number>` (`overlay::active`, swr.md §9), in `submitted_at` order.

It applies each mutation's `patch_json` in that order with `apply(raw, patches)`, which is this module's own function (swr.md §9). The result is:

```rust
pub struct EffectivePr {
    pub row: PullRequestRow,          // the raw row, unchanged
    pub detail: Option<Detail>,       // the heavy part, None before the first hydrate
    pub state: PrState,               // effective, with the patches applied
    pub checks: Checks,               // effective
    pub review_decision: ReviewDecision,
    pub title: String,
    pub head_oid: String,
    pub feedback_open: Option<usize>, // from detail; None before the first hydrate
    pub overlay: Vec<MutationId>,     // the patches applied, in order (for `cpd swr show`)
}
```

The names are this page's; the shape is the contract. `EffectivePr` is the only type a reader of GitHub state uses. **Every consumer calls it**: the E8 rules `pr-ready`, `pr-closed`, `review-requested` and `deploy` (contract §2.2), `cpd pr view`, `cpd inbox claim` (its live check, §9), `cpd pr merge` and `cpd pr post` (their prechecks), the TUI tree and `cpd plan show`. A consumer that reads `pull_requests` directly for a GitHub value is a defect of this design, and a test pins it (§11).

`effective_pr` reads no clock and no GitHub. It takes no `now` because nothing it returns depends on time: the freshness of a value is `fetched_at` and `stale_after`, which the reader returns as data where a caller needs them (`cpd swr show`).

## 7. Rate limits, outages and the endpoint `github`

### 7.1 Classification

Every failed call of the endpoint `github` (the document, a REST call of §8, a write of §9) is classified once, from what the call returned and nothing else (the rule of contract §7.6 step 4, kept):

- **Transient:** a timeout of the call's own deadline; an HTTP 5xx as `gh` prints it (`HTTP 5\d\d`, or the nginx HTML page that GitHub returns with it); a reply cut off so that `gh` cannot parse it. It feeds `endpoint::record_failure(Kind::Transient)`.
- **Permanent:** any other failure (a 4xx, a GraphQL `errors` array without `RATE_LIMITED`, an unparseable body that was not cut off, a missing field). It feeds `record_failure(Kind::Permanent)`.
- **Rate limit:** a GraphQL `errors` entry with `type` `RATE_LIMITED`, or `rateLimit.remaining` under `RATE_FLOOR`. It is §7.2, not a failure.

### 7.2 Rate limits: `swr_endpoints('github')`

The two rules that were in the engine's memory move into the endpoint row, so every GitHub reader and writer respects them:

- **The floor.** `RATE_FLOOR` is `100` (`engine/src/watch/pr.rs:35`, moved to `core/src/swr/source/github.rs`). After a pass, `endpoint::record_rate(remaining, reset_at)` stores `rate_remaining` and `rate_reset_at`, and when `remaining < RATE_FLOOR` it pauses the endpoint until `reset_at` (`pause_until`). This replaces the check at `engine/src/watch/pr.rs:337` (`if remaining >= RATE_FLOOR`). It corrects swr.md §7, which paused only at `remaining` 0; this is a newer decision and it wins (§14 records the superseding note).
- **The hard pause.** A `RATE_LIMITED` reply pauses the endpoint until `reset_at` plus `RATE_GRACE` (3 s), or `RATE_FALLBACK` (60 s) when no reset is known (`engine/src/watch/pr.rs:130-143`, `37` and `39`). It is `endpoint::pause_until(reset_at + grace)`. It is not an error, and it raises no Event: it writes one `watch.log` line, as today. The fallback when the reset is unknown is kept as it is.

A paused endpoint is skipped by the pass, by E24 for its own sources and by the outbox drain (§9). A success after the pause has passed clears the pause (`record_ok`, swr.md §7).

### 7.3 One port for `gh`

Every `gh` call goes through `core::host::runner::Runner` (the port of [contract §6.3](../contract.md)), passed explicitly. Three call sites do not yet:

- `head_ref_oid` (`core/src/pr/gh.rs:646-655`) calls `run(cwd, args)`, which takes the runner from the ambient `Ports::current()`, not from its caller. It takes `&dyn Runner` as an argument (spec item 5).
- `core/src/inbox/team.rs:287` calls `Command::new("gh")` directly. The team lookup dies with this design (§12); its GitHub read is the pass.
- `core/src/prune/mod.rs:179` calls `Command::new("gh")` directly. It is not an SWR reader. The GitHub Workflow moves it onto the port, because rule 7 of the spec has no exception.

The rule is enforced by a test that greps the source for `Command::new("gh")` outside `core/src/host/runner.rs` and `core/src/pr/gh.rs` (§11), in the form of a test, not a comment.

## 8. Deploys: `swr_deploys`

Deploys are the GitHub Actions runs that wait for the User's approval (contract §2.2 "The deploy rule"). They replace `run/deploy-runs.json` (§12).

For each repository whose policy declares `[deploys] workflows` (contract §8.3 "The `deploys` domain"), and each declared workflow file:

1. **The list** (REST, one call per declared file, through the port): `gh api "repos/<owner>/<repo>/actions/workflows/<file>/runs?status=waiting&per_page=100"`. It returns `id`, `updated_at`, `display_title`, `html_url`, `created_at` and `triggering_actor.login` per run. The list cannot be made a delta: it is the only way GitHub lists the waiting runs.
2. **The actor filter**, case-insensitive, against `swr_viewer.login` (the viewer read of the document, so no second `gh api user` call).
3. **The delta.** A run is **changed** when no `swr_deploys` row has its `(repo, run_id, updated_at)`. Only changed runs get the next call:
4. **`pending_deployments`** (REST), per changed run: `gh api repos/<owner>/<repo>/actions/runs/<id>/pending_deployments`. A run is kept when at least one entry has `current_user_can_approve` true; its `environment` is those entries' names, sorted, joined with `, `. An unchanged run keeps the approvals it had.
5. **The rows.** A kept run is written to `swr_deploys` (`run_json`, `pending_json`, `updated_at`), and its `inactive_at` is NULL. A run of the file that is no longer waiting, and a row whose pull request the header reads as `closed` or `merged`, gets `inactive_at` = `now`.
6. **Garbage.** `gc::sweep` deletes an inactive deploy row seven days after `inactive_at` (`swr.gc_days`, §5 of swr.md; A4), when no observer holds it.

A failed call of one file leaves that file's rows as they were, and the others are updated (the rule of contract §7.6 step 8, kept). The approve of a deploy is an outbox op (§9, `approve`).

## 9. Writes: the outbox ops

A write command enqueues a row of `swr_mutations` and returns `{queued, mutation_id}` with the effective state (A6). The engine drains the outbox (swr.md §12) through `Source::execute` of this module, one row at a time, through the port, and reports the outcome with `complete`, `fail` with `Retry::After` or `fail` with `Retry::Never`.

**Target keys.** A pull request op's `target_key` is `github:pr:<owner/repo>#<n>`. The deploy approve's is `github:deploy:<owner/repo>#<run_id>` (the one key that is not a pull request; decision, §14).

**Drain-time precondition.** A write's precondition is read at enqueue from the effective state, and checked again at drain against the row the pass before the drain settled. No drain makes an extra read of GitHub for a precondition: GitHub itself enforces the one a write needs (`--match-head-commit` for a merge, the viewer's own permission for a review). A precondition that fails at drain is `fail` with `Retry::Never` and an `error` Event, once (swr.md §8).

| `op_kind` | `target_key` | `vars_json` | Optimistic patch (`patch_json`) | Coalesce (swr.md §8) | Drain (`execute`) | Done when |
|---|---|---|---|---|---|---|
| `merge` | `github:pr:…` | `{"expected_head_oid", "method"}` | `state` `merging` (hides the `pr-ready` item of the pull request) | `Dedupe` on the canonical vars | `gh pr merge <n> --repo … --<method> --match-head-commit <expected_head_oid>`; the method is the first allowed of swr_repos' flags (squash, merge, rebase), as contract §9.10 | the pass shows `state` `MERGED` in the raw row (below) |
| `auto_merge` | `github:pr:…` | `{"method"}` | `auto_merge` `requested` | `LatestWins` | `enablePullRequestAutoMerge` with `mergeMethod` (the post's rule, contract §7.6 "Auto-merge"); a refusal is `Retry::Never`, except a refusal of a clean pull request (below); a failure `Retry::After` | the pass shows `autoMergeRequest` set, or the clean fallback (below) has enqueued the `merge` op |
| `approve` (review) | `github:pr:…` | `{"event": "APPROVE"}` | `review_decision` `approved` | `Dedupe` | `gh api --method POST repos/…/pulls/<n>/reviews` with the review body | the pass shows `review_decision` `approved` |
| `approve` (deploy) | `github:deploy:…` | `{"run_id", "environments"}` | effective deploy `approving`: the `deploy` item closes at once (contract §2.2) | `Dedupe` | the approval POST of contract §21.13 for the run's approvable environments | the run is gone from the waiting list (§8) |
| `reply` | `github:pr:…` | `{"to", "body"}` | `feedback` without the item `to` (the item leaves at once) | `Dedupe` on `(to, body)` | `addPullRequestReviewThreadReply` for a `thread`; `addComment` with the `Re: <url>` prefix for `comment` and `review` (contract §9.8a) | the pass shows the reply's marker in the feedback read, which leaves the item out |
| `describe` | `github:pr:…` | `{"body"}` | none (the description is cpd's stored value, read from `workflows`) | `LatestWins` | `gh api -X PATCH repos/…/pulls/<n> --input -` with the body | the pass shows the new `updatedAt` |
| `retitle` | `github:pr:…` | `{"title"}` | `title` `<new>` | `LatestWins` | `gh api -X PATCH …` with the title | the pass shows the title |
| `rerun` | `github:pr:…` | `{"run_ids": [databaseId…]}` | `checks` `pending` | `Dedupe` | `gh run rerun <id> --failed`, in order; stops at the first failure, which is `Retry::After` | the pass shows the checks of the head moved (`pending` or `passed`) |
| `post_comment` | `github:pr:…` | `{"body"}` | none | `Dedupe` on the body | `addComment` with the body verbatim (no `Re:` prefix) | the pass shows the new comment |
| `claim` | `github:pr:…` | `{"item_id", "claimed_head_oid"}` | `claiming` (the `review-requested` item shows `claiming`) | `Dedupe` on the item id | the claim's Effects (contract §21.7), run by the engine, with the live check read from the effective row of the pass before the drain | the `inbox-claimed` Event is written; the item closes as claimed |
| `post` (Slack) | `github:pr:…` | `{"channel", "text", "kind": "post" or "repost"}` | `posting` (the Posting state of the Inbox row) | `LatestWins` | the Slack send of contract §9.8b and §9.8b2; **owned by the slack Workflow's `execute`**, this page owns only the target key and the patch (assumption, §14) | the send's answer is stored (`pr_posts` row for `post`) |

**A clean pull request falls back to merge.** GitHub refuses `enablePullRequestAutoMerge` on a pull request that is already in clean status (its message says "Pull request is in clean status"): there is nothing for auto-merge to wait for. So when the `auto_merge` drain is refused and the stored `pull_requests.merge_state` is `clean`, the op does not fail. It settles `done` and enqueues the `merge` op in its place, through the same enqueue as `cpd pr merge` (contract §9.10), so the containment gate runs there: a head behind the default branch is queued for an `update` Job and armed, as `head_behind_main` says. The decision reads the stored `merge_state`, never the refusal's message text. No `swr_mutation_failed` Event is raised for this refusal. A refusal while the stored `merge_state` is not `clean` keeps the table's rule (`Retry::Never`, one error Event).

**Confirmed causal transitions wait for the raw cache.** A transition that changes other rows, namely the unstack of a dependent branch, the Plan's `done` and the poll's `pr-merged` (contract §3.8, §3.1), runs only when the **raw** `pull_requests` row says `merged` (the `state` of §6's `row`, not the effective one). The overlay `merging` is never enough. The raw row says it when a pass that started after the merge commit has read it (the fence). A merge whose pass never proves it leaves those transitions to the next pass that does.

**A failed write's overlay goes at once** (swr.md §8: `fail` with `Retry::Never`). That is a true revert: the item returns to its raw state, and an error Event says why.

**Writes in a pass.** A write is not a read; it is not counted by the "one document" rule (§3.1). A drain is its own call. The pass and the drain run in one engine step (E5, then the drain of §4 step 10), and neither starts while the endpoint is paused.

## 10. Guarantees

These are the guarantees of the spec's point 5. Each names its mechanism and its test (§11).

**(a) Leads are told from the cache.** `checks_failed`, `changes_requested` and `comments` are decided in the commit transaction of the pass, from the raw rows it just wrote (contract §7.6 step 6, unchanged in content). A Lead that is woken reads the same cache that `cpd pr view` reads, so what it is told and what it can see agree.

**(b) The review Lead is woken on a new head.** A Review Lead is the Lead of a Plan whose `reviewed_repo` and `reviewed_number` name a pull request that some `inbox_items` row has `claimed_at` set for. Today only a User's re-claim wakes it (contract §2.2, commands §21.7, the reused-Plan path). The new rule: **in the commit transaction that stores a `head_oid` change, or a `checks` change to `passed`, of such a pull request, one `session-message` Event is appended to the Lead** (`data.from` null, `wake` true, the form of contract §3.7 E20 and E21) when all of these hold:

- the claimed item's `head_oid` (the head at claim time, `inbox_items.head_oid`) differs from the stored new `head_oid`;
- the new `checks` is `passed`;
- no `session-message` of that Plan has `data.head_oid` equal to the new head: this is the idempotence key, so one head wakes once, and a second change of checks on the same head wakes nothing.

Its text names the pull request's URL, the new head and the claimed head, and tells the Lead to re-review. **Where the rule lives:** here, and in [attention-inbox.md](attention-inbox.md) "The reviewed PR of a review Plan" (the wake), with commands §21.7 and contract §7.6 pointing to it.

**(c) Managers see each Workflow's PR state.** `cpd plan show` prints each Workflow's `pull_request` from `effective_pr`: `checks`, `review_decision`, `feedback_open` and `head_oid` (added to the printed row by this design; commands §4.3). A Manager reads it from the cache with no call.

**(d) Inbox items do not flicker.** The items of E8 for CI, review, mergeable and deploy are computed from `effective_pr` only. Their value changes in two ways only: a Changed commit of the raw row, which is a real change of GitHub's state, and an overlay's start or end, which is a real change of intent (a write queued, or a fence settled or a write failed). A pass that changes nothing writes nothing (D5), so an item's colour does not alternate between two passes. The identity rule of contract §2.2 still applies: a colour change closes and opens the item, and a change of label inside a colour updates it in place.

**(e) Deploys.** `swr_deploys` holds the waiting runs; the reads are delta-only (§8); `approve` is an outbox op whose overlay closes the item at once (§9); a row goes inactive when its pull request is closed or merged, and `gc` deletes it seven days later (§8, `swr.gc_days`).

**(f) Open-only.** The rule of §4.2: a merged or closed pull request has `stale_after` NULL and is read only by the one confirmation read of a locally open pull request that left the open list, or by a Slack first-seen read. An archived repository's pull requests are not candidates.

**(g) The Issues seam.** `gh_issues` exists (the swr-core migration) and nothing reads it. A Rust constant, `GH_ISSUES_SEAM: &[&str] = &["issues_alias"]` in `core/src/pr/gh.rs`, lists the seam. The builder carries `issues_alias(repo, number) -> String`, a pure function that returns the alias text for one issue (`i<n>: repository(owner, name) { issue(number: <n>) { number state title updatedAt } }`). It is unit-tested and never called by a pass, so a later Plan that polls issues adds a call site and no new query shape.

## 11. Tests

Every test mocks `gh`, the node helpers and time through `FakeRunner` and `ManualClock` (`core/src/host/runner.rs:220`, `core/src/ports.rs`, the `Fakes` of `core/src/ports.rs:77-118`). No test runs a real process, network, sleep or tmux (the policy of [tests](../../CONTEXT.md)). Each unit test sits beside its module as `<name>_tests.rs`.

The tests this design needs, by the rule they pin:

- **Document shape** (`pass_document_tests.rs`): the builder emits the viewer, rate limit, the two searches, the `Heavy` set exactly for moved or unhydrated rows, the cap of §3.2, `authoredMore` only after a `hasNextPage` true, and `issues_alias` appears in no document.
- **One call per pass** (`pass_tests.rs`): a pass with twenty candidates makes one `FakeRunner` call of the endpoint.
- **Open-only** (`candidates_tests.rs`): a merged or closed row is never an alias; a locally open row absent from the lists is read once; an archived row is not a candidate.
- **Rate** (`rate_tests.rs`): `remaining` 99 pauses until `resetAt`; a `RATE_LIMITED` pause lasts `resetAt + 3 s`, or 60 s with no reset; a paused endpoint makes no call and no drain.
- **Failures** (`classify_tests.rs`): a timeout, an HTTP 502 and a cut-off reply are transient; a 4xx is permanent; a failed pass writes nothing (no header, detail, flag or deploy row).
- **Reader** (`github_tests.rs` in `core/src/swr/`): `effective_pr` applies the active patches in `submitted_at` order; a failed mutation contributes nothing; a `revalidating` one contributes until settled.
- **Fence** (`settle_tests.rs`): a merge whose pass started before its `completed_at` leaves the overlay; the raw `MERGED` is the only trigger of the causal transitions.
- **Single readers** (`readers_tests.rs`): no module outside `core/src/swr/` reads `pull_requests` for `checks`, `review_decision`, `state` or `head_oid` except through `effective_pr` or the pass; a grep test over the source fails on `Command::new("gh")` outside `core/src/host/runner.rs` and `core/src/pr/gh.rs`.
- **Wake** (`wake_tests.rs`): a head change with green checks on a claimed pull request appends one `session-message`; the same head again appends none; a head change with checks not green appends none.
- **Deploys** (`deploys_tests.rs`): an unchanged `(run_id, updated_at)` makes no `pending_deployments` call; a row whose pull request closed goes inactive.
- **Checks reduction** (`status_tests.rs`, existing): `ChecksSummary::of` keeps the check-level `excused_labels` reduction of pro-d447 (A9). Its tests stay green. This branch predates the #920 merge on `main`; the rebase is the Job's first step, as the spec says.
- **Cutover** (per file, §12): each deleted cache has no reader after its cutover, pinned by a test that the file path no longer appears in the source.

## 12. What dies

Each row is removed in the same change as its replacement, with no flag and no dual write (swr.md §16).

| Deleted | Where it is | Replaced by |
|---|---|---|
| the static `GITHUB` memo of E8 | `core/src/inbox/rules.rs:49`, used at `:88` | `effective_pr`, read per tick from SQLite |
| `team::GITHUB_TTL` and its `Memo` | `core/src/inbox/team.rs:27`, `:392-420` | nothing: E8 reads the rows the pass wrote |
| the team lookup's own `gh api graphql` call | `core/src/inbox/team.rs:264-288` | the pass's document: E11's team lookup reads GitHub only through the single poll |
| `run/team-review.json` | `core/src/inbox/team.rs:69` (the writer and the reader) | `swr_slack_posts` (the slack Workflow) |
| `run/deploy-runs.json` | `core/src/inbox/rules.rs:151`, its writer in the poll, and the rewrite of `inbox approve` | `swr_deploys` (§8) and the `approve` op (§9) |
| `run/pr-poll-failing.json` | `engine/src/watch/pr.rs:228-240`, `:190-297`; `engine/src/watch/report.rs:309-336` | `swr_endpoints` `failing_since` and `reported_at` (§7.1) |
| the RATE_FLOOR check and the rate pause in engine memory | `engine/src/watch/pr.rs:35`, `:130-143`, `:337` | `swr_endpoints` `record_rate` and `pause_until` (§7.2) |
| the E18 queue use of `pr_posts.auto_merge` | the E18 step's selection of `pr_posts` rows with `auto_merge` `pending` | the `auto_merge` op and its retry in `swr_mutations` (§9). The column stays, as the stored history of a post |
| the TUI's in-memory Posting set | `tui/src/widgets/mod.rs:567-572` | the overlay `posting` of the `post` op (§9) |
| the merge marker file | `tui/src/merge/marker.rs` | the `merge` op's status (`in_flight`, `revalidating`) |

`run/pr-poll-now` (the immediate-poll request, contract §1.4) is **not** in this table: its replacement is open (§14).

## 13. Assumptions and decisions of this page

These are added to [swr.md §17](swr.md). Each is stated so a later Job can test or overturn it.

- **G1. One document per pass, the candidates decided before the call.** The alias set is computed from the rows of the previous pass, so one call can carry it. Consequence: a pull request that left the open list is confirmed one pass later. (The spec's "absent from the searches" is read as "absent from the previous pass's searches" for this reason.)
- **G2. The heavy cap of 25 and the `first` counts** (§3.1, §3.2) are guesses, unverified against GitHub's node limit.
- **G3. Light fields added** to the spec's list: `labels`, `autoMergeRequest`, `latestOpinionatedReviews`, `reviewThreads(first:100)` for owned pull requests (§3.1, the reasons given there).
- **G4. The cursor** for `authoredMore` needs a text column that no `swr_*` table has (§3.3). Open: the pass sends the first page only until it is decided (§14).
- **G5. The deploy approve's target key** is `github:deploy:<owner/repo>#<run_id>` (§9).
- **G6. The Slack `post` op** is owned by the slack Workflow's `execute`; the github Workflow owns its target key and its `posting` patch (§9). The spec lists `post_comment` as the GitHub op; no command enqueues it yet, and it is listed so the set matches the spec (§9).
- **G7. The rate floor pauses**: `remaining < 100` pauses until `resetAt`, which corrects swr.md §7 (§7.2).
- **G8. A write's precondition is read at enqueue and at drain from the settled cache row**, with GitHub enforcing its own (§9). This is a choice: a write that needs a fresher read than the last pass gives is refused by GitHub, not by cpd.
- **G9. `head_oid` and `gh_updated_at`** are the migration's columns, written by the pass (§2). `pull_requests.updated_at` keeps its meaning.

## 14. Open items

Not decided by this page. Each is recorded so no Job guesses it.

- **Which writes stay synchronous.** `pr open`, `pr push`, `pr rebase`, `workflow add --adopt` and the codereview `review submit` call `gh` and push git branches. This design enqueues the writes of §9 only. The Lead decides whether the git writes (a push and a Gate cannot be a row without their own design) and the review submit join the outbox.
- **`run/pr-poll-now`** (contract §1.4, the immediate-poll request, pro-d392, pro-d394): its replacement is `cache::invalidate` on the `github:pr` keys plus a wake, which is the swr.md §16 rule. Not decided here; the Job that cuts over E5 decides it.
- **`pull_requests.updated_at`** (pro-d68) is superseded by `swr_pr_detail.updated_at` for the heavy trigger. Dropping it is a breaking migration; not decided here.
- **Storing `rateLimit.cost`** (§5): logged for now.
- **The paging cursor** (§3.3, G4): a text column for the second search page. Decide whether the github Workflow's breaking migration adds `swr_queries.cursor`, or the page is skipped.
- **The superseding notes of this Job** (rule 4 and G7). swr.md §7 and contract §3.7 E5 say the old rule. They are annotated, not rewritten: the newer decision stands and the older text is marked superseded (AGENTS, "Recency wins").
- **`pr view` when no detail is hydrated yet**: the design prints `detail` `null`. Whether the Lead should wait for it is not decided.
- **`prune/mod.rs:179`** calls `gh` directly (§7.3). It is not an SWR reader; moving it is in this Plan's scope by rule 7, but its owner Job is not named here.
- **The `triggering_actor.login` of the deploy list** is compared case-insensitively against the viewer's login (§8). Assumption from contract §7.6 step 8; verified only against the live example of 2026-10-27 that the contract cites.

## Decision pro-d453 (Light read, Danger freeze)

- A teammate's open pull request whose stored rollup is `failed` and whose labels (`labels_json`) match an excused label is hydrated (the `Heavy` set, within the cap) although it is not the User's own, and once for its head: the contexts are what excuse a failing Danger. The Light read has only the rollup state, so a later light node of the same head reduces the stored detail's contexts again under the policy hold instead of bringing `failed` back.
- `poll_node` computes the policy hold once (`status::policy_hold`) and passes it to `ChecksSummary::of`, which excuses a failing `danger` CheckRun and a failing `Danger` StatusContext. The hold needs the excused label: a failing Danger without it stays failed (pro-d447).
- The Inbox reader derives `checks_state` from the effective `checks` first (`passed` is `SUCCESS`, `failed` `FAILURE`, `pending` `PENDING`); only a row with `none` falls back to the detail's rollup. It takes the reviews from the Light read's `latestReviews` and the author from `author_login`.
