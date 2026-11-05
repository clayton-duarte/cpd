# Inbox click errors and stale `ready to merge` rows

Research only, read on `origin/main` at `5702ea1d8` (2026-11-05 EST). Every `path:line` is that tree. Nothing here is implemented.
Claims I did not run are labelled *inferred*.

User report (2026-11-05 EST): a PR row read `ready to merge` (yellow) while its Workflow was updating (a base PR had just merged and GitHub was recomputing mergeability). Merge errored, the row stayed. User rule: **whenever a click on an Inbox item produces an error, that error must update the item's state or message.**

Note on wording: the source never says "not CLEAR yet". The refusal is `merge_not_clean`, quoted in (1).

## (1) A Merge click, widget to `gh`

| Step | Where |
|---|---|
| Menu offers Merge only when the row's label status is `ready to merge` (`Entry::Merge`, `Entry::Merging` while a marker is live) | `tui/src/widgets/inbox.rs:420-440` (`Entry::entries`), status parsed from the label by `Status::of_item` `core/src/inbox/pr.rs:85-91` |
| Pick closes the menu, redraws, runs the entry on a `Background` job (pro-d350) | `tui/src/widgets/mod.rs:523-551` (`background.submit`), `Background::on` `:413` |
| `InboxEntry::Merge` → `Pull::from_subject` → `crate::merge::start(&opener.tmux, ..)`; an `Err` of *starting* is `dialog::report(.., "Merge Pull Request", &e)` | `tui/src/widgets/mod.rs:532-544` |
| `start` lists tmux clients, runs `tmux run-shell -b "cpd tui action merge '<ref>' --client <name> 2>/dev/null"` and returns. The job ends here; **it does not wait for the merge** | `tui/src/merge/mod.rs:69-83`, `start_args` `:88-100` |
| `cpd tui action merge` → `merge::action_with` (cockpit dispatch `"merge" => Action::Merge` `tui/src/cockpit.rs:440`, call `:531`): marker file, then `cpd pr merge <ref>` in process | `tui/src/merge/mod.rs:129-160` |
| `pr merge` → `merge()`: `gh pr view <n> --repo <repo> --json state,mergeStateStatus,headRefOid,title,mergedAt` (live) | `core/src/pr/merge.rs:75-80`, `core/src/pr/gh.rs:553-586` (`MergeView`, `view_for_merge`) |
| Refusal | `core/src/pr/merge.rs:84-96` |
| then required checks (`refuse_unless_required_passed`, `gh pr view --json statusCheckRollup`), merge methods (`gh repo view`), `gh pr merge` | `core/src/pr/merge.rs:97-110`, `:132-172`, `gh::merge_pr` |
| success: marker kept as merged, `cpd pr poll`, marker dropped when the row is no longer open | `tui/src/merge/mod.rs:161-182` |

The refusal, verbatim (`core/src/pr/merge.rs:84-96`):

```rust
    if view.state != "OPEN" || view.merge_state != "CLEAN" {
        return Err(CpdError::new(
            ErrorCode::MergeNotClean,
            format!(
                "{repo}#{number} is {} and {}: only an OPEN pull request that is CLEAN is merged.",
                view.state, view.merge_state
            ),
        )
        .with("repo", repo.as_str())
        .with("number", number)
        .with("state", view.state)
        .with("merge_state", view.merge_state));
    }
```

Code `merge_not_clean`, class `Refused`, exit 3 (`core/src/error.rs:251`, `:549`). Rendered, as the tests pin it: `merge o/r#7: merge_not_clean: o/r#7 is OPEN and BLOCKED: only an OPEN pull request that is CLEAN is merged.` (`cli/tests/tui/tui_merge.rs:290`). Docs: `docs/commands.md:3039` (table row), `:160` (code row), `:4408` (the failure rule).

The refusal is read **before** any write: `merge()` writes only when GitHub reads `MERGED` (`core/src/pr/merge.rs:81-83`, `store_merge` `:174-200`) or after a successful merge (`:112-116`). A refusal writes nothing.

## (2) Where the error goes, and whether the TUI parses it

- `cpd tui action merge` runs with stderr dropped (`2>/dev/null`, `tui/src/merge/mod.rs:90`).
- On failure `action_with` builds `merge <pull>: <code>: <message>` from the `CpdError` (`tui/src/merge/mod.rs:185-187`), appends it to `$CPD_HOME/run/tui/merge.log` (`:188`), then opens the one failure dialog on that client, `dialog::show_error_on` (`:191`, `tui/src/dialog/mod.rs:761-768`). Dropping the marker before the dialog (`:186`) ends the spinner.
- **Not an Event, not a toast, not stored.** Nothing in `core/src/pr/merge.rs` appends an Event on refusal; `inbox_items` is untouched. pro-d373: engine/click errors stay in Events or logs, not in Inbox rows (the decision is about `engine-error` items).
- **The TUI does not parse the code.** It formats `code()` and `message()` into text (`tui/src/dialog/mod.rs:773-780`, `report`/`report_why`). No branch on `merge_not_clean`.
- Every other click goes through the same `dialog::report` (log line `failed: <what>: <code>: <message>`, then dialog): `Opener::choose` (approve, claim, snooze, dismiss) `tui/src/widgets/mod.rs:381-391`; `Opener::post`/`repost` → `pr_command` `:316-369`; `jump` `:273-305`; link open `:596-600`; Copy Link `:372-378`. `dialog::report` `tui/src/dialog/mod.rs:773-790`.
- After any background job ends, `background.finished()` sets `redraw` and `schedule.force()` so the Inbox is re-read (`tui/src/widgets/mod.rs:441-444`). That re-read only shows what E8 already stored; a refused click changes nothing in the store, so the re-read shows the same row.
- There is no "close" menu entry: the entries are Approve, Claim, Snooze 15m/1h/tomorrow, Dismiss (`tui/src/widgets/inbox.rs:347-399`) plus Merge, Post, RePost, Go to Lead, Copy Link (`:404-470`). Items close only through E8 or inside `inbox claim` / `inbox approve` (5).

## (3) How a PR row's status is derived, and why `ready to merge` shows while `updating`

**Source columns.** `core/src/inbox/pr.rs:173-264` (`conditions`) reads, per open authored `pull_requests` row (`pulls::COLUMNS`, `core/src/pr/pulls.rs:58-59`): `draft`, `mergeable`, `merge_state`, `checks`, `review_decision`, `base_ref`, `default_branch`, `feedback`, plus, from joins: `owned` (a Workflow owns it), `posted` (`pr_posts`), `overdue`, `clean` = `ready_to_merge_sql` (`core/src/pr/pulls.rs:51-56`: open, not draft, `merge_state = 'clean'`, `base_ref = default_branch`) and `handled` (`core/src/inbox/pr.rs:184-187`): `sessions.state = 'working'` **or** a `jobs` row of the owner Workflow with `kind IN ('update','work')` and `status IN ('queued','running')`. No `gh` call: contract §2.2 line 722.

**Derivation.** `classify`, `core/src/inbox/pr.rs:133-169`, first match wins (docs: `docs/contract.md:733-746`):

1. `draft` → Draft.
2. broken (`mergeable = 'conflicting'` or `checks = 'failed'` or `review_decision = 'changes_requested'`) → `handled` ? Updating (conflict) / Fixing : Conflict / Failing / ChangesRequested (`:138-147`).
3. `checks = 'pending'` → ChecksRunning (`:148-150`).
4. ReadyForReview (`:153-156`).
5. `clean && (approved || none) && (...)` → **ReadyToMerge** (`:157-160`).
6-8. Stacked / InReview / Waiting.

**`handled` is read only in row 2.** Rows 3 to 8 ignore it. `Updating` is reachable only through a *stored* `mergeable = 'conflicting'`.

**Refresh.**
- `pull_requests` is written by `cpd pr poll`: E5 runs it as a subprocess every `watch.pr_poll_seconds`, default 5 (`core/src/config/file.rs:172,399`; `engine/src/watch/pr.rs:49,59`), skipped while GitHub's rate limit is nearly spent (`engine/src/watch/pr.rs:34`, pro-d384 cause 1). It lists all open PRs (`search_open`, `core/src/pr/poll.rs:115`) and stores `mergeable`/`merge_state` from GraphQL `mergeable mergeStateStatus` (`core/src/pr/gh.rs:812`, `MergeState::of_graphql` `core/src/pr/status.rs:362`). GitHub computes mergeability lazily, so a poll just after a base merge can read the old `CLEAN` or `UNKNOWN`.
- The Inbox row is a snapshot: E8 (`engine/src/watch/inbox.rs`) runs each tick (`watch.tick_seconds`, default 5000 ms, `core/src/config/file.rs:398`), calls `reconcile` (`core/src/inbox/model.rs:491-`): a colour change closes the item and opens a new one; a label change in a colour updates the row in place (`:506-521`).
- cpd's own push resets the row: `reset_after_push` sets `checks='pending', mergeable='unknown', merge_state='unknown'` (`core/src/pr/pulls.rs:357-369`; callers `core/src/merge/mod.rs:193`, `core/src/pr/push.rs:78`, `core/src/pr/rebase.rs:466`; pro-d384; contract `:5596`). **Only at push.**

**Why the yellow row coexisted with an updating Workflow** (*inferred* from code; I did not reproduce it):
1. The base PR merges. The poll sees the PR repointed to the default branch and, in the same transaction, queues the `update` Job (`core/src/pr/poll.rs:593-607`, `queue_update_for` `:913-944`, Cause `BaseMerged`). The row now has `base_ref = default_branch`, so it is no longer Stacked.
2. The stored `merge_state` is whatever the poll read while GitHub was still recomputing. If that was `clean` (and `checks` `passed`, `mergeable` not conflicting), `clean` is true.
3. The update Job is `queued`, so `handled` is true. But no push has happened yet, so `reset_after_push` has not run: the stale `merge_state='clean'` stays.
4. `classify` skips row 2 (not broken), row 3 (not pending) and lands on **row 5, ready to merge, yellow** (`core/src/inbox/pr.rs:157-160`). `handled` is not consulted.
5. Merge re-reads GitHub (`merge.rs:80`), which now says e.g. `BLOCKED`/`BEHIND` → `merge_not_clean`. The dialog shows; the stored row is unchanged, so the yellow row stays until a later poll rewrites `merge_state`.

So: **precedence row 5; stale input `pull_requests.merge_state = 'clean'` (and `checks = 'passed'`), plus `handled` ignored outside the broken branch.** A `merge_state = 'unknown'` row is already *not* ready: `ready_to_merge_sql` needs `'clean'` (`core/src/pr/pulls.rs:53`); pinned by `an_unknown_mergeable_is_neither_failing_nor_ready` (`core/src/inbox/rules.rs:1523`).

## (4) On-demand refresh, and where merge reads live state

- `cpd pr poll`: arguments are only `--plan <id>` (`PollArgs`, `core/src/pr/mod.rs:140-144`; `PlanArg` `core/src/args.rs:23-27`). It polls **all** the User's open PRs, whatever `--plan` says (docs `docs/commands.md:2419`: "one poll of all of the User's open pull requests"). **There is no single-PR refresh.**
- Cost: takes `locks/pr-poll.lock`, waits up to 60 s (`core/src/pr/poll.rs:41`, `take_lock_with` `:356`); one `search_open`, then `due_reads` comment reads (GraphQL, `:204`, `:1086-1103`) and check reads; it can hit `gh_rate_limited` (pro-d384, pro-d385). The merge action runs it once after a *successful* merge only (`tui/src/merge/mod.rs:164`).
- Live reads that already exist: `pr merge` (`gh::view_for_merge` `core/src/pr/gh.rs:569-586`, `checks_for_merge` `:589-611`, `merge_methods`, `delete_branch_on_merge`); `inbox claim` (`team::github_lookup_with`, `core/src/inbox/cmd.rs:557`, pro-n345); `cpd pr view` (`core/src/pr/view.rs`, not read in depth).

## (5) Existing ways an action failure changes a row, and refusal codes

`inbox_items` columns (as used in `core/src/inbox/model.rs:529`, `cmd.rs`): `id, kind, urgency, subject, label, url, context, plan, job, session, record, event, opened_at, closed_at, snoozed_until, dismissed_at, claimed_at`. **No error or message column.**

Mechanisms that exist:
- **`inbox claim` closes the item when GitHub says it is stale**, then refuses `not_claimable`: `core/src/inbox/cmd.rs:570-596` (`model::close_item(tx, &current, ..)` at `:572-578`; error built `:586-596`). This is the one precedent of a refusal that changes the row, and it works because `claim` reads live state.
- `inbox approve` closes the item on success (`cmd.rs:481-488`), not on failure.
- Snooze and dismiss set `snoozed_until` / `dismissed_at` and append an Event (`cmd.rs:373-440`).
- `pr merge` closes the `pr-ready` item and stores `merged` only when GitHub reads `MERGED` (`core/src/pr/merge.rs:174-200`, `close_pr_ready`).
- E8 reconciles from stored `pull_requests`, jobs and sessions only; no Event of a click failure feeds it. No error marker, no failure-driven snooze.

Refusal codes:

| Command | Code (exit) | Source |
|---|---|---|
| `pr merge` | `usage` (2) bad ref `merge.rs:37-41`; `merge_not_clean` (3) `:84`; `required_check_not_passed` (3) `:160`; `no_merge_method` (3) `:101`; `gh_failed` from the gh calls | `core/src/error.rs:251-253`, `:549-551` |
| `pr post` | `workflow_not_open` `post.rs:49`; `already_posted` `:293`; `post_disabled` `:70`; `not_authored` `:83`; `post_unconfigured` `:101`; `pr_not_ready` `:112`; `no_merge_method` `:126`; `policy_invalid` `:142`; `spawn_failed` `:162`; `store_failed` `:209`, `:271` (message WAS posted) | `core/src/pr/post.rs`; `core/src/error.rs:316-328`; docs `docs/commands.md:137-141,181` |
| `pr repost` | `post_not_found` `repost.rs:40`; `spawn_failed` `:65`; `store_failed` `:86` | `core/src/pr/repost.rs` |
| `inbox claim` | `item_not_found` (`cmd.rs:276`), `item_closed` (`:281`), `not_claimable` (`:528`, `:588`), `post_not_found` (`:540`) | `core/src/inbox/cmd.rs`; `error.rs:246-250` |
| `inbox approve` | `item_not_found`, `item_closed`, `not_approvable` (`cmd.rs:455`), `bad_row` on an invalid deploy subject (`:471`), `gh` errors from `deploys::approvable` / `approve` | `core/src/inbox/cmd.rs:451-488` |
| `inbox dismiss` | `item_not_found`, `item_closed` (`cmd.rs:401-415`, `open_item` `:289-296`); already dismissed is success with `already: true` | |
| `inbox snooze` | same two, plus the `date` read `spawn_failed` (`cmd.rs:306-310`) | |
| close | no Inbox command or menu entry closes an item on request (see (2)) | |

## (6) Tests, and the `scripts/tests.sh` name grammar

Merge refusal path:
- `cli/tests/workflow/pr_merge.rs:279` `a_pull_request_that_is_not_open_and_clean_is_refused_before_any_merge` (OPEN/BLOCKED, DIRTY, UNSTABLE; MERGED/CLEAN, UNKNOWN; CLOSED/CLEAN → `merge_not_clean`, exit 3, details `{repo, number, state, merge_state}`, one `gh pr view` call).
- `cli/tests/workflow/pr_merge.rs:524` `a_precheck_that_reads_merged_stores_the_merge_and_closes_its_pr_ready_item_hs_d526`.
- `cli/tests/workflow/pr_merge.rs:239` `a_repo_that_allows_no_merge_method_is_refused_before_any_merge`.
- `cli/tests/tui/tui_merge.rs:288` `a_refused_merge_opens_the_failure_dialog_on_the_client_logs_it_and_runs_no_poll`.
- `tui/src/merge/mod.rs:219-` unit tests (argument, quoting, `start_args`).

`pr-ready` status derivation:
- `core/src/inbox/pr_tests.rs`: `an_owned_broken_pr_is_cyan_while_its_lead_works_or_an_update_or_work_job_is_live` (`:220`), `a_pr_with_pending_checks_is_a_cyan_checks_running_row` (`:277`), `ready_to_merge_for_a_post_owner_needs_a_post_or_an_approval` (`:349`), `the_states_take_the_documented_precedence` (`:401`), `a_clean_approved_unowned_pr_is_a_yellow_ready_to_merge_row` (`:513`), `stack_review_stacking_blocks_only_ready_to_merge` (`:606`).
- `core/src/inbox/pr.rs:297-359` unit tests (`the_texts_are_distinct_and_the_urgencies_follow_the_table`, `of_item_round_trips_every_status_and_refuses_the_rest`).
- `core/src/inbox/rules.rs:1523` `an_unknown_mergeable_is_neither_failing_nor_ready`.
- Reset after push: `core/src/pr/push_reset_tests.rs`.
- Menu: `tui/src/widgets/inbox.rs` (menu entries) and `tui/src/widgets/approve_tests.rs`.

Name grammar of `scripts/tests.sh` (`scripts/tests.sh:1-13` → `cpd test`, `core/src/testsel/mod.rs:355-410`; docs `docs/commands.md` §26): `scripts/tests.sh run <name>...` and `list <name>`. A name is classified by `kind_of`: **web** `web/<file>.test.ts(x)::<title>`, **pi** `pi/test/<file>.test.ts::<title>`, else **Rust**, "a Rust name as nextest filters it" (a test-function name or path substring, not a file path). `list` exits 0 only when that one test exists. Tests are listed by the **function name** above, which must be unique enough for the filter.

## (7) Proposal

### A. Smallest change: a refused Merge updates the row (cause: the row holds an input the click just disproved)

`pr merge` already holds the live `mergeStateStatus` at refusal. Store it, so E8 re-derives the row on its next tick (at most one `tick_seconds`, default 5 s). No schema change, no new status, no TUI change.

- `core/src/pr/merge.rs`, `merge()`: before returning `MergeNotClean` for an `OPEN` PR, call a new `store_refused_state(&repo, number, &view.merge_state)`, modelled on `store_merge` (`:174-180`): `Store::open(...)` and `store.write("pr merge", ..)`. It runs `UPDATE pull_requests SET merge_state = ?1, changed_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE repo = ?2 AND number = ?3 AND state = 'open' AND merge_state <> ?1`, with `?1` = `MergeState::of_graphql(&view.merge_state)?.as_str()` (`core/src/pr/status.rs:362`; today `pub(super)`, reachable from `pr::merge`, which is in the same module tree). An unknown GitHub word is `gh_failed`, as in the poll. `MERGED` keeps `store_merge` (`:81-83`); other states (`CLOSED`) are left to the poll.
- `pr merge` must still return the same error (the dialog and `details` are unchanged); a failed store is added to the error as a detail, never swallowed.
- Effect on the report: a PR now stored `blocked` / `behind` / `unknown` is not `clean`, so `classify` leaves row 5, E8 relabels in place or closes and reopens by colour (`core/src/inbox/model.rs:498-545`), and the row stops reading `ready to merge`.
- Poll interplay: the next E5 poll overwrites `merge_state` from GraphQL. If GraphQL still says `CLEAN` while `gh pr view` says otherwise, the row flips back (*inferred*; the two reads use different fields). That is the argument for B.
- Docs to amend with the code: `docs/commands.md` §9.10 (preconditions row `:3039`: "a refused `OPEN` pull request stores the `merge_state` it read"), `docs/contract.md` §2.2 (the rule text near `:722`), the "A failed merge" bullet `docs/commands.md:4408`.

New tests (each fails first on `origin/main`):
1. `cli/tests/workflow/pr_merge.rs`: `a_not_clean_refusal_stores_the_merge_state_it_read` (OPEN/BLOCKED view, stored row `clean` → `blocked` after the refusal; error code and details unchanged).
2. `cli/tests/workflow/pr_merge.rs`: `a_not_clean_refusal_of_a_pull_request_with_no_stored_row_writes_nothing`.
3. `core/src/inbox/pr_tests.rs`: `a_stored_blocked_merge_state_is_not_a_ready_to_merge_row` (pins the contract the fix relies on, `merge_state` `blocked`/`behind` → not yellow `ready to merge`).
4. `cli/tests/tui/tui_merge.rs`: extend `a_refused_merge_opens_the_failure_dialog_on_the_client_logs_it_and_runs_no_poll` with "and stores the state" or add `a_refused_merge_leaves_the_stored_state_of_the_read` (the dialog and the log are unchanged).

Other clicks, same rule: a `Post` refusal (`pr_not_ready`, `post_disabled`, `already_posted`, …) is decided from the **stored** row and policy, so the row already shows the cause; the real gap is `store_failed` after a send. `claim` already closes on a stale PR (`cmd.rs:570-596`). `approve`: a `gh` failure leaves the row (correct, the deploy is still pending). Dismiss/snooze: a refusal means the item is closed or missing, and E8 has already closed it. So the generic rule needs only the Merge change, plus the user's choice below on a visible message.

### B. Root cause: `updating` or `unknown` must never read `ready to merge`

`unknown` already cannot: `ready_to_merge_sql` needs `merge_state = 'clean'` (`core/src/pr/pulls.rs:53`). The hole is a **live `update` Job on a clean-looking row**.

1. `core/src/inbox/pr.rs`: split `handled` so `Facts` also carries `updating: bool` = a `jobs` row of the owner Workflow with `kind = 'update'` and `status IN ('queued','running')` (a new column in the SQL at `:184-187`). `classify` gets a row between 2 and 3: **`updating` and not broken → `Status::Updating`** (cyan, label `updating`, already in `Status::ALL`, `:36-49`). It does not use "Lead working" (a Lead works on most PRs; that would hide real `ready to merge` rows).
2. Same transaction as the queue, so no stale-clean window exists at all: `core/src/pr/poll.rs`, in the base-merge branch (`:603-607`), when `queue_update_for` returns `Some`, call `pulls::reset_after_push(tx, plan, workflow)` (`core/src/pr/pulls.rs:357`). Caveat, **unverified**: the poll writes the polled row elsewhere in the same pass; the order must be checked so the reset is not overwritten by the stale `clean` the poll just read. If it is, B.1 is the only fix needed and B.2 is dropped.
3. Docs: `docs/contract.md` STATUS table (`:733-746`: a new row 2b), `docs/commands.md` §13.2, `pi/skills/cpd-lead/SKILL.md` (pro-d158 lists it as a place the status table is restated; not read here).

New tests:
- `core/src/inbox/pr_tests.rs`: `a_clean_pr_with_a_live_update_job_is_a_cyan_updating_row_not_ready_to_merge` (cases: update `queued`, `running` → cyan `updating`; update `merged`/`failed`/`cancelled`, a `work` Job, a working Lead → yellow `ready to merge`).
- `core/src/inbox/pr_tests.rs`: `an_unknown_merge_state_is_never_a_ready_to_merge_row` (every status word except `clean`; with `checks` `passed` and approved).
- `core/src/pr/poll_tests.rs`: `a_base_merge_that_queues_an_update_job_resets_the_stored_merge_state` (only if B.2 holds).
- Update `the_texts_are_distinct_and_the_urgencies_follow_the_table` only if a status is added (it is not).

### Open choices for the User

1. **Visible error on the row, or state only?** A only makes the row stop lying. To show the *message* (`merge_not_clean: ... BLOCKED`) on the row you need a stored place: a new nullable `inbox_items.note` (migration, set by a click's failure, cleared at the next reconcile), or the label's Row 2. pro-d373 keeps errors out of the Inbox as items; this would be a per-item note, not an item. Choose: state only (A), or add the note column.
2. **Refresh after a refused click:** store the one read `pr merge` already made (A), or also run the whole `cpd pr poll` as the success path does (`tui/src/merge/mod.rs:164`). The poll is not per-PR, can wait 60 s for its lock and costs GraphQL (pro-d385); I do not recommend it.
3. **Add `pr poll --pr <ref>`** (a single-PR refresh)? It does not exist (4). It would serve all clicks, but is new surface; A does not need it.
4. **B.1 scope:** `update` Job only (proposed), or `update` and `work` Jobs (more rows read `updating`).
5. **B.2:** reset the stored row when the poll queues the `update` Job (pro-d384 extended to the poll's trigger), or only B.1.
6. **Post `store_failed`** (message sent, store write failed): leave as a dialog, or record the post so the row reads `waiting for peer review`.
