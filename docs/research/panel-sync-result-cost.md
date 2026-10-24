# Panel sync: idle cost and write load, before and after

Job `final-cost2`, Attempt 1, Plan `sync` (decision hs-d595 part 3). Interim commit: round 1 of 3 measured, the
tables are being filled from `/tmp/sync-probe-cost2/out/results.jsonl`.

## 1. Method

- BEFORE `4a071cf5` (first parent of the merge of PR sync, `b9b5338b^1`), AFTER `d5bf3c42` (tip of origin/main, contains
  the merge of PR views `bd179cbd`). Release builds (`scripts/cargo.sh build --release -p cpd`), one `git worktree` and one
  `CARGO_TARGET_DIR` each under `/tmp/sync-probe-cost2`.
- Each run: a fresh `mktemp -d` home, a private tmux server `sync-probe-cost2-<pid>-<n>` (`-f /dev/null`), the seed of
  `panel-sync-latency.md` (Lead idle), the three views and the engine, 15 s settle, then the window.
