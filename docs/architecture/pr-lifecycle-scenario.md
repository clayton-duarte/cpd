# PR lifecycle scenario

Status: **the expected behaviour** of an authored pull request from open to merged, and the invariants the GitHub SWR pass and the update Job queue must keep. It is the contract the executable scenario tests (`workflow::pr_lifecycle_scenario`, Job `autonomy-tests`) pin. It is a design page, not a test run: each transition's "At HEAD" column records what the code does as **read** in `core/`, `engine/` and `docs/`, and is marked where it was not run.

Decisions: pro-d554 (behind-base is not the User's concern; only a conflict matters, and the Workflow resolves it), pro-d518 (the Inbox shows a pull request only when a human is needed), pro-d549 (the Workflow is the plan; its Jobs are registered at once). Bugs: b31 and b32 (the Lead is not woken on comments; a ready PR stays `waiting`), b33 (the SWR pass drops feedback), b34 (a behind-base compare failure writes NULL, and no update is queued). Related: [swr-github.md](swr-github.md) (the one-document pass), [attention-inbox.md](../contract.md#inbox-item) (the `pr-ready` item), [contract §7.6](../contract.md) ("Pull requests and the PR rebase") and [§3.8](../contract.md) (Workflow statuses).

## 1. The nine transitions

The brief's sequence has fourteen steps. They are nine transitions, one per test, in this order. The arrows in brackets are the steps each one covers.

| # | Transition [steps of the sequence] | Must hold | Test | At HEAD |
|---|---|---|---|---|
| T1 | **open → CI fails** [open → CI fails] | The `open` Job merges the pull request (§3.8 `dispatch → open`). The next pass stores `checks` `failed` from the one document's rollup. Failing CI does not change the Workflow's status. | `t1_open_ci_fails` | Header stores `checks` (`write_header`, `swr_read.rs:14`). Read, not run. |
| T2 | **CI fails → Lead woken** [CI fails → Lead woken] | Exactly one `pr-feedback` Event with reason `checks_failed` to the Plan's Lead, when `checks` becomes `failed` from anything else. It needs the Workflow's `pr.feedback.checks` to be `any` (`attention`, `pulls.rs:487`). | `t2_lead_woken_on_failing_check` | Rule holds (read). Not run. |
| T3 | **Lead woken → push → CI green** [Lead woken → push → CI green] | A new head resets `checks` to `pending` (contract §7.6 "The reset"). The pass on green stores `passed`. Green raises no `pr-feedback`. Readiness clears on the first pass after green, given P1 to P3 below. | `t3_push_ci_green` | `checks` path holds (read). Readiness depends on T4's broken write (below). |
| T4 | **CI green → bot comments → Lead woken** [CI green → bot comments → Lead woken] | The SWR pass stores the open review threads and comments into `pull_requests.feedback` (P1). Each new item raises one `pr-feedback` `comments` Event to the Lead (P6). Readiness shows `bot_comments` while a bot item is open (P8). | `t4_bot_review_comments_wake_lead` | **Broken** (b33, b31). The pass applies its effects with empty comment reads (`poll_pass(.., Some(vec![]))`, `poll.rs:177`, `:279`), so `read_of` finds no read and `upsert` keeps the stored feedback, which stays `NULL` (`pulls.rs:344`). Read; the store counts are the scout's and not re-run. |
| T5 | **Lead woken → replies → comments clear** [replies] | `pr reply --to <item>` posts the reply. The item leaves `feedback` at once (the overlay of swr-github §9, `reply`). The next successful read without it keeps it out. Readiness clears when no bot item is left. The reply never resolves a thread (contract §9.8a). | `t5_replies_clear_comments` | Blocked by T4 at HEAD (read). The reply's own path not run. |
| T6 | **conflict → Lead rebases (pro-d554)** [conflict → Lead rebases] | When `mergeable` is `conflicting` (or `merge_state` `dirty`), one `update` Job is queued for the Workflow, cause `conflict` (Q1, Q2). The Lead is told. No Inbox item, no label, no escalation to the User or the Manager (Q4). The Job merges the base, resolves the conflict, runs the Gate and submits; cpd pushes (contract §7.7). Behind-base without conflict is not this transition (Q3). | `t6_conflict_with_main_queues_update` | Queueing holds (`update_for_conflict`, `poll.rs:1336`; `open::queue_update`, `open.rs:300`). The Lead is told by a session message (`update_for_conflict`, `poll.rs:1336`), not by a `pr-feedback` reason (G4). No Inbox item (`inbox/pr.rs`, row 9). Read, not run. |
| T7 | **ready → one Inbox item** [ready → 1 Inbox item] | When the PR is open, not a draft, owned, not posted, its post enabled, and `unready` is empty (no failed or pending checks, no `changes_requested`, no conflict, `feedback` persisted and no bot item), exactly one `pr-ready` Inbox item shows, with status **"ready for user review"** (yellow). Not two for one pull request. | `t7_ready_raises_one_inbox_item` | Status rule holds (`classify`, `inbox/pr.rs:140`). **Broken for any PR with `feedback` NULL**: `unready` gives `CommentsUnread` and the item stays `waiting` (b32, `postable.rs`). |
| T8 | **ready → peer review requested → approved** [1 Inbox item → peer review] | After `pr post`, the Workflow has a `pr_posts` row and its item reads "waiting for peer review". A peer's approval raises one `pr-feedback` `approved` Event to the Lead, and the item becomes "ready to merge" (yellow). | `t8_peer_review_requested_and_approved` | Rules read (`classify`, `attention`). Not run. |
| T9 | **merged → closed** [peer review → merged → closed] | The pass sees `MERGED` and stores `merged`. Its `pr-ready` item closes in the same transaction (swr-github §4 step 6). The Workflow moves `open → merged` (§3.8). The Plan is `done` when its last Workflow merged. | `t9_merged_closes_workflow` | Read: `merge()` (`poll.rs:1442`), `close_pr_ready` (`inbox/model.rs:413`). Not run. |

## 2. Invariants

### 2.1 SWR pass: comments and feedback persistence

These hold for every pass, not only for the scenario. `feedback` is the open items of a pull request as of its last successful read; `NULL` means never read.

- **P1. What the pass reads, it stores.** A successful heavy read of an open pull request stores its open review threads and comments in `pull_requests.feedback`, in the same transaction as its header (swr-github §4 step 5). **HEAD: violated** (b33). The pass hands the legacy path an empty reads list, so the stored feedback is never written from the SWR read.
- **P2. Never read is unknown, and it is read again.** `feedback` `NULL` is "never read", not "no comments". An owned open pull request whose `feedback` is `NULL` is read on the next pass, whatever its `updated_at`. **HEAD: the legacy `due_reads` rule skips it** (`poll.rs:1531`; test `null_feedback_with_an_unmoved_updated_at_is_not_due`). That is the second cause of b31. The fix must not reintroduce it.
- **P3. A header write never clears feedback.** `write_header` calls `upsert(.., None)` (`swr_read.rs:14`), and `upsert` keeps the stored `feedback` when no read is given (`pulls.rs:344`). A value a read stored survives every header write. **HEAD: holds** (read).
- **P4. A failed read writes nothing.** A failed call leaves every cached row, feedback included, as it was (swr.md §6; `GithubSource::failed`, `github.rs:298`). **HEAD: holds** in the code shape (read; not run).
- **P5. Unchanged writes nothing (D5).** A read whose items equal the stored items opens no write transaction (swr-github §4 step 5). **HEAD: holds** for the header (`upsert` compares values, `pulls.rs:315`).
- **P6. One Event per new item.** An open item whose `last` id is new to the stored feedback raises one `pr-feedback` `comments` Event, in the transaction that stores it. An item already stored raises none (`comments::new_items`, `comments.rs:329`). **HEAD: holds** (read).
- **P7. A closed thread leaves feedback on the next successful read.** Resolving a thread does not move `updatedAt` (pro-d231), so the read must not depend on it. **HEAD: holds in the legacy rule** (`stored_thread_closed`, `poll.rs:1522`); the SWR path's rule not read.
- **P8. Readiness reads feedback.** `unready` gives `CommentsUnread` for `NULL` and `BotComments` for a bot item (`postable.rs`). A pull request is ready only with `feedback` persisted and no bot item. **HEAD: holds** (read).

### 2.2 Update Job queueing, containment and the Inbox

- **Q1. A conflict queues one update Job.** When `mergeable` is `conflicting` (or `merge_state` is `dirty`), the row's `state` is `open`, the Plan is not archived or held, and the Workflow has no `update` Job `queued` or `running`, the poll queues one `update` Job with cause `conflict` (`update_for_conflict`, `poll.rs:1336`; `open::queue_update`, `open.rs:300`). **HEAD: holds** (read).
- **Q2. Level-triggered, at most once per head.** A conflict that still stands queues again when no update Job is live, and at most once per `head_oid` (contract §7.6 rule 3; pro-n442). **HEAD: gap** (G3). `update_for_conflict` keys on no head: `apply` calls it for an unchanged stored row too (`poll.rs:870`), so it queues on every pass that finds the row conflicting and no live Job.
- **Q3. Behind base with no conflict queues nothing.** A mergeable pull request whose `merge_state` is `behind` (or whose `base_contained` is 0) gets no update Job from the poll. A base that merely moved is no cause (contract §7.6 rule 3; hs-d326, hs-d497; pro-d554). It is updated only when `pr merge` arms it, which raises `Trigger::Behind` (commands §9.10). **HEAD: holds** (read; the Lead's answer on the question of this page).
- **Q4. No Inbox item for behind base, and none for a conflict unless escalated.** pro-d554. **HEAD: holds** (read: `inbox/pr.rs`, row 9 and the `conflicting` branch, which `continue`s unless escalated).
- **Q5. A failed containment compare keeps the last known value.** `base_contained` changes only on a successful compare: `ahead` or `identical` is 1, `behind` or `diverged` is 0. A failed or missing compare leaves a known value as it is, and never writes NULL over a known 0 or 1 (b34). The Lead decided this on the question of this page. It supersedes swr-github.md §1, which writes NULL, and that page needs a superseding edit (not in this Job's files). **HEAD: broken** (G2). The containment loop runs on every pass, not only the legacy path. It writes `.ok().map(i64::from)` (`poll.rs:495`), so a failure becomes NULL.
- **Q6. An unknown flag is never ready.** `ready_to_merge_sql` requires `base_contained = 1` (`pulls.rs:57`). **HEAD: holds** (read).

## 3. Gaps at HEAD

Each gap is read in code. None is run, and the scout's store counts are not re-measured here.

- **G1 (b33, b31, b32).** The SWR pass never writes `feedback` (P1, P2). Cause: `poll_pass` runs with empty reads from `apply_pass_effects` (`poll.rs:177`). The scout's count of open pull requests with `feedback` NULL (47 of 48) is unverified here.
- **G2 (b34).** A failed containment compare writes NULL over a known value (Q5, `poll.rs:495`). Cause: the `.ok()` in the containment loop. The scout's claim that the compare fails for #2444 is unverified (no network).
- **G3.** Conflict queueing has no head key (Q2, `poll.rs:870`). Standing conflicts re-queue whenever no update Job is live.
- **G4.** A conflict wakes the Lead by a session message, not by a `pr-feedback` reason `conflicting` (contract §7.6 rule 3, pro-d518). `attention` names no conflict reason (`pulls.rs:487`, citing hs-d340).
- **G5 (tests).** `github_pass_tests` asserts `head_oid` only, so none of P1 to P3 or Q5 has a test (scout §4). The scenario tests are the guard.

## 4. Assumptions

- The nine transitions are the nine tests `t1` to `t9` of Job `autonomy-tests`, in order. The brief's fourteen steps are grouped into them as the table shows.
- "The Lead rebases (pro-d554)" is read as the Workflow's `update` Job: its Worker merges the base, resolves the conflict, runs the Gate and submits, and the Lead is told (contract §7.7, `open.rs` Cause `Conflict`). The Lead does not merge by hand. The Lead's answer to the behind-base question described the same mechanism.
- "ready for review" in the brief is the Inbox label **"ready for user review"** (`inbox/pr.rs`, `Status::ReadyForReview`).

## Sources

- [contract.md](../contract.md) — §3.8 (Workflow statuses), §7.6 (the poll, rule 3 of "Ready, never a draft"), §7.7 (update Job), §9.8a (`pr reply`), §9.10 (`pr merge`).
- [swr-github.md](swr-github.md) — §1 (containment), §4 (the pass), §9 (outbox ops), §10 (guarantees).
- [attention-inbox.md](../contract.md#inbox-item) — the `pr-ready` item.
- Records: pro-d554 (2026-11-11 UTC), pro-d518 (2026-11-10 UTC), pro-d549 (2026-11-10 UTC), pro-n442.
- Code read at HEAD: `core/src/pr/poll.rs`, `core/src/pr/pulls.rs`, `core/src/pr/postable.rs`, `core/src/pr/swr_read.rs`, `core/src/swr/source/github.rs`, `core/src/inbox/pr.rs`, `core/src/job/open.rs`.
