# Leaked cpdt tmux servers (leak-docs, 2026-10-23)

## Summary

The three `cpdt-*` servers leaked because the test `tui::a_failing_action_prints_nothing_exits_0_and_tells_the_client` (its `NEXTEST_TEST_NAME` is in all three environments) called a bare `kill-server` while the `pane-died`/`window-unlinked` hooks were armed, so a hook ran `cpd watch --ensure --wait` and started a new server in `/tmp/tmux-1000` after the env's directory was removed (hygiene.md bugs 3a, 3b, 3c).
**No gap remains** on this path: #223 (dd9c7131, 2026-10-21 13:12) fixed it, and every leaked server started before that (11:51, 12:11, 12:50).

## Evidence

`ps -o pid,lstart,ni,args -p <pid>` (ppid 1, `tmux -u -L <name> -f /dev/null … new-session -d -s cpd-work -n watch … -- cpd watch`):

| server | pid | started | nice | `CPD_HOME` | made by (`CARGO_MANIFEST_DIR`) |
|---|---|---|---|---|---|
| cpdt-3215531-0 | 3219404 | Oct 21 11:51 | 0 | /dev/shm/cpd-test-nR6qGi/cpd-home | cpd.hygiene.lifecycle.gate-2616431 |
| cpdt-758318-0 | 768142 | Oct 21 12:11 | 0 | /dev/shm/cpd-test-xAQ7St/cpd-home | cpd.hygiene.sleep.gate-415843 |
| cpdt-109395-0 | 113637 | Oct 21 12:50 | 19 | /dev/shm/cpd-test-xO4ull/cpd-home | cpd.ui-design--open-pills-a1 |

- `tmux -L <name> list-panes -a -F '#{pane_id} #{pane_pid} #{pane_dead} #{pane_current_command}'` printed `%0  1 cpd` for all three: `pane_dead=1` (the `cpd watch` is dead; `remain-on-exit on` keeps the pane).
- `/proc/<pid>/environ` of each server: `NEXTEST=1`, `NEXTEST_BINARY_ID=cpd::tui`, `NEXTEST_TEST_NAME=tui::a_failing_action_prints_nothing_exits_0_and_tells_the_client`, `TMUX_TMPDIR=/dev/shm/cpd-test-<id>/tmux`; no `GATE_RUN` (the Gate's names carry it only since #223). The sockets are in `/tmp/tmux-1000`, not under that `TMUX_TMPDIR`: tmux fell back to `/tmp` because the directory was gone (3b).
- The three Gate worktrees ran the Gate before #223 (`git log -S GATE_RUN` finds only dd9c7131, 2026-10-21 13:12 -0400).
- The test ends with `env.kill_tmux_server()` (`cli/tests/tui/tui.rs:497`); before #223 that was a bare `kill-server`. hygiene.md R3a measured 5 of 40 runs leaking a server.
- The pane cwd of each server was `<dir>/home (deleted)`: nothing else held the `/dev/shm/cpd-test-*` directories (`ls -l /proc/*/cwd | grep cpd-test`, `fuser`).
- Other tests that start a tmux server through `TestEnv` and `cpd watch`: `flow/plan.rs:420`, `engine/watch.rs:670`, `tooling/harness.rs:594`, `tui/tui.rs:497` all call `kill_tmux_server()`, which unsets both hooks first. `TestEnv::new` also starts a reaper (`TEARDOWN_SH watch <test pid>`) that tears the env down when the test process dies.

## Reproduction

Current main (09b72595), `TMPDIR=$(mktemp -d /dev/shm/leakrepro-XXXX)`, the one test above, `GATE_RUN` unset:

- `scripts/cargo.sh nextest run -p cpd --test tui -E 'test(=tui::a_failing_…)' --stress-count 20`: 20 passed; afterwards no new server, no new socket, no `/dev/shm/cpd-test-*`, `$TMPDIR` empty.
- The test binary alone (`target/debug/deps/tui-… --exact tui::a_failing_…`) under `timeout -s TERM` and `timeout -s KILL` at 0.3, 0.6, 1.0 and 1.4 s (a run killed never runs `Drop`): the reaper tears down within 4 s every time; the same counts, `$TMPDIR` empty.
- `scripts/gate-run.sh scripts/cargo.sh nextest run … --stress-count 20` (`GATE_RUN` set): exit 0, nothing left.
- A Worker's targeted run is outside the Gate's scan, but it needs none for this test: the reaper and the unset hooks do the work.

Not proven: a test that kills a server with a bare `env.tmux(&["kill-server"])` while cockpit hooks are armed (`tui/tui_worker.rs:300` does a bare `kill-server`; its server is not the watch-supervised one, unverified). The leftovers of my runs were removed.

## Fix

None needed: the root fixes are on main (3a `kill_tmux_server`; 3b `Tmux::output` refuses a missing `TMUX_TMPDIR`, `core/src/host/tmux.rs:518`; 3c `watch --ensure` refuses a missing `$CPD_HOME`). What proves it: no `cpdt-*` server started after 13:12 on Oct 21 exists (the three were 11:51–12:50), and 40 stress runs and 8 kills left nothing. A guard against a new bare `kill-server` would be a test that greps `cli/tests` for it; not built (YAGNI).

## Cleanup

- `tmux -L cpdt-109395-0 kill-server`, `-L cpdt-758318-0`, `-L cpdt-3215531-0` (after `unset TMUX`).
- Sockets `/tmp/tmux-1000/cpdt-{109395-0,758318-0,3215531-0}` and the dead `cpdt-rANdEU-1-0`.
- `/dev/shm/cpd-test-{nR6qGi,O3xbOe,xAQ7St,xO4ull}` (no process had one as cwd after the servers died).
- `/dev/shm/leakrepro-*` (my scratch TMPDIR). `tmux -L cpd ls` still lists `cpd` and `cpd-work`.
