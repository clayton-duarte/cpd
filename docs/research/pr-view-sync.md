# Do the Plan view and the Deploy column stay stale after a PR merges? (hs-n86)

Job `rs-stale-views`, Plan `cross-repo`, item C1. Code read on `main` at `5dcb3f1`.

## Answer

**Not stale on `main`: both views show the merge within about 2 s of the `pr poll` that records it.** hs-d159 (#20 fixed it) holds for what I could reproduce. One residual way to see a stale PR remains, and it is outside the views: nothing polls when the engine (`cpd watch`) is not running. I did not reproduce hs-n86 in the harness and have no recorded cockpit capture of it, so its original cause is unverified (guess: the engine was off, or the binary in the cockpit predated #20/#16).

## What was run

A throwaway test (copy of `cli/tests/tui_plan.rs`'s helpers; not committed):

1. Store: one Context, Plan `ship` in phase `deploy`, Lead in the chat slot, one PR `repo` (`prs.status = open`) with an open `pull_requests` row, acme/repo#7, checks `passed`, and one merged Job.
2. A private tmux server (`env.tmux_socket()`) with `cpd tui view plan` and `cpd tui view widgets` in two windows, 260×40.
3. A fake `gh` answering `pr poll`: the open search first lists #7; then it lists nothing and the lookup of #7 answers `MERGED`.
4. `cpd pr poll` once; captures with `tmux capture-pane -p` after 4 s and 8 s. Bounded waits only; the server was killed (`kill-server`) at the end.

| | before | after the poll (+4 s and +8 s) |
|---|---|---|
| Plan view (box title) | `┌ repo · repo → main · #7 open ┐` | `┌ repo · repo → main · #7 merged ┐` |
| Deploy column (PRs section) | `acme/repo#7` / `open ✓ no review  PR 7` | `no open PRs` |

`pr poll` printed `merged: [{plan: ship, pr: repo}]`, `done: [ship]`. Events appended: `pr-merged` (id 2), `plan-done` (id 3), both with `plan = ship`; then an `error` `cleanup_failed` (id 4, only because the fixture's repo `/repo` does not exist; not related to the views).

## Why it works (file:line)

- What a merge writes, in one transaction (`core/src/pr/poll.rs`): `apply` upserts the `pull_requests` row to `merged` (`poll.rs:155`, `pulls::upsert`), then `merge` sets `prs.status = 'merged'` (`poll.rs:193`) and appends `pr-merged` with `plan` set (`poll.rs:202`), and `plan-done` when it was the last PR. A PR no Plan owns gets only the row change and no Event (hs-d96).
- Plan view (`tui/src/plan/mod.rs`): `concerns` (`:93-97`) is true for any Event whose `plan` is the shown Plan's, so `pr-merged` and `plan-done` mark the view stale; the loop (`:242-262`) re-reads every `events.poll_ms` (200 ms) and retakes the snapshot after `COALESCE` (500 ms). The box state comes from `read::plan_boxes` → `PrState::of(status == Merged, row_state)` (`tui/src/read/flow.rs:222`, `tui/src/read/shared.rs:149`), drawn by `state_text` (`tui/src/plan/boxes.rs:67`).
- Deploy column (`tui/src/widgets/mod.rs`, `show`): re-reads when the highest Event id moved, and in any case every `REFRESH` = 2 s (`:79-84`, `:36-38`), so it also catches the Event-less change of an unowned PR. The PRs section lists `pull_requests WHERE state = 'open'` (`tui/src/read/widgets.rs:170`, `open_prs`), so a row the poll set to `merged` leaves the list; an unowned PR that left the search is deleted (`core/src/pr/pulls.rs:228`, `delete_unowned_absent`).
- commands.md §17 states this design for both views and the code follows it.

## Residual gaps (not bugs of the views)

1. **No engine, no poll.** `pr poll` runs only from the engine's E5 step every `watch.pr_poll_seconds` (default 5; `core/src/watch/pr.rs:27-33`, `core/src/config/file.rs:236`) or by hand. With `cpd watch` stopped, a merged PR stays `open` in both views. Not reproduced; read from the code.
2. **Rate-limit pause.** E5 pauses until `reset_at` when GitHub's remaining requests fall under 100 (`core/src/watch/pr.rs:24,50-76`); the views stay as they were meanwhile (the engine reports `gh_rate_limited`). Read from the code, not run.
3. **Search index lag.** A merged PR that GitHub's search still lists as open is left unchanged until the index catches up (`poll.rs:62`, "the index lags (D3)"). Not run.

## Proposed Job (only if the User wants gap 1 visible; otherwise none)

No Job is needed for the question asked. If the User wants a merged PR never to look open because the engine is off:

- **Title:** `statusbar-engine-off` — the cockpit says when the engine is not polling.
- **Tier:** S2.
- **Files:** `tui/src/widgets/column.rs`, `tui/src/read/widgets.rs` (the `Engine` enum already has `Never`, `Stopped`, `Crashed`), `cli/tests/tui_widgets.rs`, `docs/commands.md` §13.2.
- **What:** the PRs section shows a dim `engine stopped: not polling` line when `Engine` is not `Running`, with a test for each state. This is speculative and optional; I did not ask the User.
