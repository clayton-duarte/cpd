# Research: tui (the cockpit's ratatui views and `cpd tui`)

Base `main` (HEAD `5ef69b2` when I started). Area: `tui/src/` except the SQL of `tui/src/read/` (Job `store-sql`).
Classes follow hs-d153: Work, then Fast, then Good. Every claim has a command, a measurement or a cited line; the
few that do not say `unverified`.

## Summary

The views are in good shape where it counts for a human: error paths restore the terminal, the tree view has
46 unit tests, I found no `unwrap` on read data, idle CPU is small (the four live views together use about 0.9 % of one
core and their memory is flat at about 12 MB each). The defects sit at the **seams**: what a view does when it ends
or fails, and what it does with a failure. Because the `pane-died` hook respawns every `@cpd_view` pane at once, `q`
in the Worker view is a restart (the hook half demonstrated on a private tmux; a real `q` was not run), and any read error that ends a view is a respawn
with no back-off. The tree's follower can fail silently and leave the tree stale for good; the tree's Job counters
miss the two Events that move a Job between `queued` and `running`; the plan view hides a failed snapshot after
200 ms. The one measured cost worth fixing is that the **widgets and statusbar views each re-read 1.9 MB of the
database every 2 s while idle** (about 0.95 MB/s and 237 `pread`s/s each, measured on the live views) to draw one
number and a list of pull requests; tui-dots will add a 2 s tree re-read of 2.4 MB per snapshot (a projection). Redraws are not coalesced either: every idle tick writes 25 bytes to the pty. The structure findings
are mostly duplication (the scroll code is copied three times, nine copies of the terminal set-up) and some dead code
the build already warns about.

## All findings

| id | class | what | size |
|---|---|---|---|
| tui-W1 | Work | A view that exits is respawned at once by the `pane-died` hook: `q` in the Worker and widgets views is a restart; a failing start or read is a respawn loop with no back-off; the tests run without the hook | Job |
| tui-W2 | Work | Tree follower: a failed spawn is swallowed (`.ok()`), leaving the tree stale and silent for good; it spawns `current_exe()`, which reads `… (deleted)` after an install | Job |
| tui-W3 | Work | The tree follows neither `attempt-started` nor `attempt-retry`, the Events that move a Job `queued`↔`running`, so its counters lag | Job |
| tui-W4 | Work | No shared error policy: the plan view shows a failed snapshot for 200 ms and never retries; the tree never retries and keeps a stale note; widgets, statusbar and logs exit | Job |
| tui-W5 | Work | `cpd tui` never rebuilds a missing pane (other than the StatusBar) and does not finish an aborted build, against §13.1 "each only when missing" | Job |
| tui-W6 | Work | A killed test leaks its tmux server, `tui view tree` and follower (found alive 1 h 52 min later); 42 `/tmp/cpd-test-*` directories are left | Job |
| tui-W7 | Work | The tree's UI thread blocks on subprocesses and popups (no timeout, no redraw); the plan view runs five tmux calls inline | Job |
| tui-W8 | Work | Names are cut by `char` count, not display width: wide characters overflow the counters column | Job |
| tui-W9 | Work | After a panic the pane keeps mouse reporting on: ratatui's panic hook restores only raw mode and the alternate screen | Job |
| tui-W10 | Work | The Deploy column needs the `hostname` program and dies on any load-average failure | Job |
| tui-F1 | Fast | widgets and statusbar re-read the whole Dispatch model every 2 s: 1.9 MB, 466 `pread`s each, 0.95 MB/s each when idle; the statusbar draws one number | Job |
| tui-F2 | Fast | Every view redraws on every tick (tree 10 Hz, the rest 5 Hz) although nothing changed; ratatui writes 25 bytes each time | Job |
| tui-F3 | Fast | One tree snapshot reads 2.4 MB (579 `pread`s); tui-dots will take it every 2 s, which projects to +1.2 MB/s | Plan (with F1) |
| tui-F4 | Fast | The tree's follower is a whole `cpd` process plus a thread: 9.4 MB RSS, as much as a view | Job |
| tui-G1 | Good | The scroll/wrap/key code of the Worker and history views is a copy; the logs view has a third | Job |
| tui-G2 | Good | Nine hand-written terminal set-up/restore sites, three `terminal_failed` closures, `not_a_terminal` used for any terminal I/O error | Job |
| tui-G3 | Good | Dead code: `plan::Actions::{run_command, open_lead}` (the build warns), `read::flow()` and its types (tests only), unread fields of `Widgets` | Job |
| tui-G4 | Good | Four ways of following Events (subprocess + thread, in-process poll with a full row fetch, with `highest_id`, a 500 ms store poll) | Plan |
| tui-G5 | Good | Stale doc comments and §17 text that no longer match the code | Job |

---

## Work

### tui-W1 — Views that exit come straight back (`q` is a restart; failures loop)

- **what:** The cockpit sets `set-hook -g pane-died 'if -F "#{@cpd_view}" "respawn-pane"'` (contract §6.1). Every
  pane that runs a view carries `@cpd_view`, so when a view's program exits for any reason tmux starts it again at
  once. Three consequences:
  1. `q` in the **Worker** view ("`q` or Esc quits", commands.md §13.2) and `q` in the **widgets** view ("`q` quits")
     do not close anything: the view comes back. The Worker pane is closed only by the plan view (a second click on
     the Job). The history view knows this: on `q` it calls `chat::run()` (the placeholder) instead of exiting.
  2. A view that fails to start or hits a read error (W4) exits with an error, so the hook runs it again at once; I
     found no delay anywhere (`respawn-pane` is immediate and `pane-died` fires on every exit).
  3. The integration tests start their views in a tmux that has `remain-on-exit` but **not** the hook, so
     `cli/tests/tui_worker.rs:220-235` (`q` ends the view, `pane.dead()`) passes while the real cockpit behaves
     differently.
- **where:** `core/src/host/tmux.rs:48` (`PANE_DIED_HOOK`); `tui/src/worker/view.rs:49` (`q`/Esc return `true`);
  `tui/src/widgets/mod.rs:73-77,91-93` (`q` returns `Ok`); `tui/src/history/mod.rs:67` and `history/view.rs:50` (the workaround);
  `cli/tests/tui_worker.rs:220-235`; wrong comment `tui/src/plan/mod.rs:14` ("a layout pane that dies stays dead
  (contract §6.1)": §6.1 says the opposite).
- **evidence:** (hook half, run on a private server, not the cockpit) a pane that carries `@cpd_view` and whose
  program exits every 2 s, with the hook set as `PANE_DIED_HOOK` sets it:
  ```
  S=quality-hook-$$; tmux -L $S -f /dev/null new-session -d -s t "sh -c 'echo started; sleep 2'"
  tmux -L $S set -g remain-on-exit on
  tmux -L $S set-hook -g pane-died 'if -F "#{@cpd_view}" "respawn-pane"'
  tmux -L $S set -p -t t:0.0 @cpd_view worker; tmux -L $S respawn-pane -k -t t:0.0 "sh -c 'echo started; sleep 2'"
  for i in 1 2 3 4 5 6 7 8; do sleep 1; tmux -L $S display -p -t t:0.0 '#{pane_pid} dead=#{pane_dead}'; done
  → t=1 pid=1997778   t=2 pid=1999143   t=4 pid=1999817   t=6 pid=2000476   t=8 pid=2001029   (dead=0 each time)
  ```
  The pid changes every 2 s: the dead pane is respawned each time. That a real `q` in `cpd tui view worker` behaves
  the same is **unverified** (I may not run `tui`); it follows from the hook, `set_pane_view(&pane, View::Worker)` in
  `tui/src/plan/actions.rs:112` and the code above. A failing start's cost: `cpd config` takes 3.1 ms median
  (N=20, Method), so a view that fails at start would loop at up to a few hundred starts per second per view
  (**unverified**: tmux's own latency was not measured).
- **impact:** Documented keys that do nothing (Worker `q`/Esc, widgets `q`); a persistent error (a missing
  `hostname`, W10; `store_too_new` after a downgrade, hs-n81) burns CPU in a respawn loop and the User sees a
  flickering pane instead of a message.
- **proposed fix:** Decide the rule once: a view never exits on its own except to close itself. For `q` in the Worker
  view, run `tmux kill-pane -t $TMUX_PANE` (as the plan view does for the second click) instead of returning; drop
  `q` from the widgets view (the plan view has none). A view that cannot start sleeps for a few seconds and shows the
  error before exiting. Add the hook to the tests' tmux (`Tmux::apply_server_options`) so a test sees what the cockpit
  does. Fix the comment at `plan/mod.rs:14` and commands.md §13.2 (worker, widgets rows).
- **size:** Job.
- **overlap:** none in flight (tui-visuals / tui-dots do not touch the exit paths). Cross-area, one line: the hook is
  in `core/src/host/` (Job `engine`/`commands` area).

### tui-W2 — The tree's follower fails silently and spawns a path that can be gone

- **what:** `Follower::start(...)` is called as `.ok()` at both sites. If it fails (the spawn fails, or `current_exe`
  fails) the view holds `follower = None`; nothing sets `dirty` again (only a follower message does), so **the tree
  never refreshes, with no note**. The follower runs `std::env::current_exe()`; after an install (`mv -f` in
  `scripts/install-cpd.sh:40`) that reads `<path> (deleted)` and the spawn fails with `ENOENT`. `host::exe::started_as()`
  exists for exactly this (recorded at `main`, hs-d151 fix 3) and the tree does not use it.
  Second, smaller: when the follower ends at once (e.g. `cursor_ahead`, or `store_too_new` since hs-d140), `Ended` →
  snapshot → `start` → ends again: one spawn per 500 ms for as long as it persists (bounded by `SNAPSHOT_EVERY`;
  **unverified** in a running view).
- **where:** `tui/src/tree/mod.rs:75` and `:100` (`.ok()`); `tui/src/tree/follow.rs:59` (`current_exe()`);
  `core/src/host/exe.rs:72-96` (`record_start`, `started_as`), `cli/src/main.rs:5`.
- **evidence:** `ls`-free demonstration of the path rule: `cp /bin/sleep /tmp/x; /tmp/x 20 & rm /tmp/x; readlink
  /proc/$!/exe` → `/tmp/x (deleted)` (Rust's `current_exe` is that `readlink` on Linux; the install does the same
  replace). `grep -rn current_exe tui/src` → only `tui/src/tree/follow.rs:59`; `grep -rn started_as tui/src` → nothing.
- **impact:** After an install, or any failed spawn, the tree shows a frozen picture that looks live. `cpd tui
  --respawn` (run by the installer) restarts the views in the same step, which is why this is not seen in practice;
  a view alive in the gap, or an install whose respawn step fails, stays stale.
- **proposed fix:** Use `host::exe::started_as()` in `Follower::start`; on failure set `view.note` and retry on the
  next tick instead of `.ok()`. (Better still: W4/G4, drop the subprocess.)
- **size:** Job.
- **overlap:** hs-n73, hs-d140, hs-d151/hs-d152 fix 3 and Plan pi-pin PR `engine-exe` cover the **engine's** driver
  spawns and `events follow` itself; the tree's follower is not in their scope. tui-dots changes `tree/mod.rs`
  (spinner, 2 s snapshot) but not these two lines.

### tui-W3 — The tree's Job counters miss two Events

- **what:** The tree draws the five Job counters of each Plan (`queued⋅running⋅merging⋅failed⋅merged`), which change
  with the Job's status. The transition table in `docs/contract.md:479,481` says the Event of `queued → running` is
  `attempt-started` and of `running → queued` (ladder retry) is `attempt-retry`. Neither is in the tree's `KINDS`; the
  test `every_kind_followed_exists_and_the_tree_lists_of_17_are_among_them` only pins §17's list, which has the same
  gap. So a started Job shows as `queued` until the next followed Event (usually `attempt-status` `starting→running`,
  seconds later), and a retried Job keeps showing `running` until its next Attempt starts, which waits for a free slot
  (concurrency 6).
- **where:** `tui/src/tree/follow.rs:15-41` (`KINDS`); `docs/contract.md:479,481` (table); `core/src/driver/start.rs:236-257`
  (the `UPDATE jobs SET status = 'running'` and the single `AttemptStarted` Event).
- **evidence:** `grep -n '"attempt-started"\|"attempt-retry"' tui/src/tree/follow.rs` → nothing;
  `grep -n 'attempt-started` |\|attempt-retry` |' docs/contract.md` → `:479` and `:481`. Kinds in the copy: `session-state` 1621, `session-moved` 1286,
  `attempt-status` 343 of 4689, so the lag is bounded in a busy cockpit; in a quiet one it is open-ended
  (**unverified** at runtime).
- **impact:** Wrong counters (the tree's main number) for seconds to minutes.
- **proposed fix:** Add the two kinds and extend the test to "every kind that moves a Job's status or a Plan's
  phase". Alternative: follow every Event, as the plan view does, and rely on the 500 ms coalescing.
- **size:** Job (a few lines plus the test).
- **overlap:** none. tui-dots (`cpd/tui-dots/cpd`) leaves `follow.rs` unchanged.

### tui-W4 — No shared error policy: stale views with the error hidden, or views that die

- **what:** The views react to a failed store read in four different ways.
  - **plan:** a failed snapshot sets `ui.error` and `stale = false`; the very next idle fetch (200 ms later) succeeds
    and sets `ui.error = None`. The error is on screen for about 200 ms and the snapshot is never retried until
    another Event that concerns the Plan arrives.
  - **tree:** a failed snapshot sets `view.note` and `dirty = false`: no retry; the note stays after a later
    success (`refresh` does not clear it).
  - **worker:** note set, snapshot kept, retried every 500 ms (good).
  - **widgets, statusbar, logs:** the `?` ends the view with the error (then W1: respawn).
  Same code, same data: `read::widgets` also parses PR rows (`Checks::parse`, `review_decision`) that the StatusBar
  never draws, so an unexpected value in one pull-request row can end the StatusBar (see F1).
- **where:** `tui/src/plan/mod.rs:197-221` (`:201`, `:207`, `:219`, `:221`); `tui/src/tree/mod.rs:92-98`;
  `tui/src/widgets/mod.rs:80-83`; `tui/src/statusbar/mod.rs:134-136`; `tui/src/logs/mod.rs:101-102`;
  `tui/src/read/widgets.rs:189,194` (`bad_row`).
- **evidence:** by reading the lines above (unverified at runtime: provoking a read error needs a write or a `tui`
  run, both out of bounds here). Tests: `grep -n "store_failed\|note" cli/tests/tui_plan.rs` → one hit (`:335`, the "has not
  started yet" note), so no test covers a failed snapshot.
- **impact:** A transient `store_busy`/`store_failed` leaves a plan view or a tree that is out of date until the next
  Event, with the reason gone; three other views turn the same error into a restart.
- **proposed fix:** One rule, in one place (G2): a failed read keeps the last good snapshot, shows the error as a note
  until the next success, and retries after one `poll` interval. Views only exit on a terminal failure.
- **size:** Job (with G2's shared loop) or Plan (all eight views).
- **overlap:** tui-dots edits the same loops in `tree/mod.rs` and `plan/mod.rs` (snapshot due logic, spinner): land
  after it or on top of it.

### tui-W5 — `cpd tui` does not repair a cockpit that is missing a pane

- **what:** commands.md §13.1: "Effects, each only when missing: … the tree …, the plan view …, the widgets …, the
  StatusBar". `cockpit::run` builds only when `main` has **no** panes; with any pane present it respawns the *dead*
  ones and adds a missing StatusBar (amendment 15). A tree/plan/widgets/chat pane that is gone (the User ran
  `kill-pane`, or `build()` failed half-way after `create_session`) is never recreated; the call reports `built:
  false`. A failure inside `build()` (any `?` after the first pane) therefore leaves a permanent partial cockpit.
- **where:** `tui/src/cockpit.rs:57-68` (the `built` branch), `:108-129` (`build`), `:139-150` (only the StatusBar is
  added).
- **evidence:** by reading; `grep -n "kill-pane" cli/tests/tui.rs` → only the StatusBar test (`:304`); no test kills
  another layout pane or fails `build` half-way. Not run (`cpd tui` is out of bounds). **Unverified at runtime.**
- **impact:** The only way back is to kill the `main` window by hand.
- **proposed fix:** Replace "built when empty" by "for each `LAYOUT` view with no pane, add it where it goes", then
  reset the layout once (as amendment 15 does for the StatusBar); add the test.
- **size:** Job.
- **overlap:** none.

### tui-W6 — Tests leak their tmux server, view and follower when the test process is killed

- **what:** `TestEnv::drop` kills the private tmux server, so a killed test process (a Worker's timeout, `SIGKILL`)
  leaks it, with everything inside: the view with `remain-on-exit`, and its follower. The temp dirs
  (`tempfile::Builder::tempdir`, also removed only on drop) leak the same way.
- **where:** `cli/tests/common/mod.rs:262-267` (the only teardown), `cli/tests/tui_tree.rs:165-175` (`Pane::start`).
- **evidence:** `ps -eo pid,ppid,etime,args | grep cpdt-` →
  `3724650  1  01:52:27 tmux -L cpdt-3722019-23 -u set-option -g remain-on-exit on ; new-session -d -s tree -x 46 -y 16 …/cpd.dev-speed--sccache-cargo-a1/target/debug/cpd tui view tree`
  with its child `…/target/debug/cpd events follow --since 1 --kind …`; `ps -p 3722019` (the creating test
  process) → gone. `ls -d /tmp/cpd-test-* | wc -l` → 42, oldest `2026-10-16 13:29`.
- **impact:** Leaked processes and files on a shared build machine (about 13 MB RSS for the leaked tmux server, view and follower: `ps` 3832 + 5184 + 3752 kB). Cross-area: Job `tests-build` owns the harness; I report the tui instance.
- **proposed fix:** Tie the server to the test process: have the harness start it with a watchdog that runs
  `kill-server` when the test's pid is gone (a `sh -c 'while kill -0 $PPID; do sleep 5; done; tmux -L … kill-server'`),
  and sweep `cpd-test-*` directories whose creator is gone.
- **size:** Job.
- **overlap:** hs-n12 (flaky test, unrelated cause). dev-speed's `prune-worktrees.sh` is about worktrees, not `/tmp`.

### tui-W7 — The UI thread blocks on subprocesses and popups

- **what:** The tree runs each action as `cpd <args>` with `Command::output()`, no timeout, inside the event loop:
  while it runs nothing is drawn or read (keys queue, a resize is not redrawn, the follower channel is not drained).
  `Effect::Legend` blocks the same way until the popup closes (the code's own comment: "`display-popup` returns when
  the popup closes"). The StatusBar's `▤` and `?` buttons call `display-popup` inline too, so **the StatusBar stops
  updating while a popup is open** (unverified: needs an attached client). The plan view's `toggle_worker` runs
  `list-panes`, (`display`,) `split-window` and `set-option` ×3 one after another on the key thread.
- **where:** `tui/src/tree/actions.rs:197-210` (`command`), `tui/src/tree/mod.rs:125-134`;
  `tui/src/statusbar/mod.rs:116-129`; `tui/src/plan/actions.rs:76-113`.
- **evidence:** by reading. For scale, `tmux list-panes -a` takes 4.7 ms median, 5.7 ms max (N=20, private server,
  load average 12-17); `cpd config` 3.1 ms. A healthy action is a few tens of ms; a hung `tmux` or a command waiting
  on the store (`store_busy_ms`) freezes the view for as long (hang time **unverified**).
- **impact:** A frozen tree on a slow command; not a data error.
- **proposed fix:** Run the action on a thread and report on a channel (the follower already has one); at least give
  the subprocess a timeout.
- **size:** Job.
- **overlap:** none (the tree-menu Plan is done).

### tui-W8 — Names are cut by `char` count, not display width

- **what:** `truncate` counts `chars()`; the footer height counts `chars()`; the menu's width counts `chars()`.
  Session names and Plan titles are free text (pi names a Session; the User types titles), so wide (CJK, emoji) or
  zero-width characters make the cut wrong and the name runs into the right-aligned counters. `row_text` already
  measures the other parts with `Line::width()` (display width), so the two disagree inside one function.
- **where:** `tui/src/tree/rows.rs:210-218` (`truncate`, doc: "Counted in chars (names are plain text)"), `:242` (its use in `row_text`);
  `tui/src/tree/view.rs:384`; `tui/src/tree/menu.rs:141`.
- **evidence:** by reading; **unverified visually** (not rendered). A title of three CJK characters has
  `chars().count() == 3` and a terminal width of 6.
- **impact:** Cosmetic misalignment of the counters on non-ASCII titles.
- **proposed fix:** `unicode-width` is already in the tree through ratatui (`Line::width`): cut with
  `Span::width`/`ratatui::text::Line` instead of `chars()` (no new dependency).
- **size:** Job.
- **overlap:** tui-dots rewrites `rows.rs` (232 lines changed in `origin/cpd/tui-dots/cpd`): do it after.

### tui-W9 — A panic leaves mouse reporting on

- **what:** Every view enables mouse capture (tree also focus events) after `ratatui::init()` and disables them only
  on the normal path after its loop. `ratatui::init()` installs a panic hook that calls `restore()`, which is
  `disable_raw_mode` + `LeaveAlternateScreen` only. After a panic the pane's terminal still reports mouse events (and
  focus events) to a program that is gone; the respawn (W1) re-enables them, but a pane that stays dead (`history`
  has `@cpd_view` too) shows escape sequences when the mouse moves. `ratatui::init()` itself `expect`s (panics) when
  the terminal cannot be set up.
- **where:** `tui/src/tree/mod.rs:63-67`, `plan/mod.rs:166-172`, `statusbar/mod.rs:82-88`, `worker/mod.rs:66-70`,
  `history/mod.rs:57-61`; ratatui 0.30.2 `src/init.rs:554-560` (`try_restore`), `:566-572` (the hook).
- **evidence:** `sed -n 554,560p ~/.cargo/registry/src/*/ratatui-0.30.2/src/init.rs` →
  `disable_raw_mode()?; execute!(stdout(), LeaveAlternateScreen)?;`. Normal and `Err` exits are covered
  (`restore` after the loop in all nine views; see "Already good").
- **impact:** Low: the only path is a panic, and no production `unwrap` on read data was found (grep, Method).
- **proposed fix:** One guard in the shared runner (G2) that disables what it enabled in `Drop`.
- **size:** Job (with G2).
- **overlap:** none.

### tui-W10 — The Deploy column needs `hostname` and dies on a load failure

- **what:** The machine's name is the output of the `hostname` program (spawned once); the load average is
  `/proc/loadavg`, or on macOS a `sysctl -n vm.loadavg` process every 2 s. Both use `?` inside the view, so a missing
  program or an unreadable file ends the view (W1: then a respawn loop) over one decorative line.
- **where:** `tui/src/widgets/mod.rs:47` (`hostname()?`), `:83` (`load_average()?`), `:99-120`.
- **evidence:** `which hostname` exists on this machine; the failure needs a host without it (unverified which).
  `core` already depends on `nix`; `nix::unistd::gethostname` is the in-process call (the `hostname` feature would
  have to be enabled: unverified that it compiles here).
- **impact:** Low.
- **proposed fix:** Read `/proc/sys/kernel/hostname` (Linux) or call `gethostname`; on any failure draw `?` for the
  load and keep running.
- **size:** Job.
- **overlap:** tui-visuals owns the Deploy column's look, not its data sources.

---

## Fast

Measured on a copy of the store (`sqlite3 .backup` of `~/.cpd/cpd.db`: 27 MB, 4689 Events, 201 Sessions, 190 Jobs,
schema 6) with my own release build, and on the live view processes read-only. The machine was loaded (load average
12-17 while I measured), so absolute times are upper bounds; medians stayed within a few percent of the minima.

### Required measurements

**1. CPU and memory of the live view processes**, `ps -eo pid,etime,pcpu,rss,args | grep 'tui view'` (once, at the start):

```
1557230   17:20  0.2 12292 cpd tui view tree
1557235   17:20  0.1 11660 cpd tui view plan
1557240   17:20  0.2 12284 cpd tui view widgets
1557245   17:20  0.1 11792 cpd tui view statusbar
1557232   17:20  0.0  9368 /home/clayton/.local/bin/cpd events follow --since 4589 --kind context-created …   (the tree's child)
```

Idle CPU from `/proc/<pid>/stat` (utime+stime, `CLK_TCK` = 100), **N = 5 consecutive windows of about 60 s each**
(the live cockpit was quiet; resolution ±1 tick = ±0.017 %), plus an earlier single 62 s window:

| process | CPU % median | CPU % max | RSS start → end (5 min) | VmHWM = VmRSS after 17 min |
|---|---|---|---|---|
| tree view | 0.25 | 0.27 | 12292 → 12292 kB | yes |
| plan view | 0.13 | 0.13 | 11660 → 11660 kB | yes |
| widgets view | 0.25 | 0.28 | 12284 → 12284 kB | yes |
| statusbar view | 0.20 | 0.22 | 11792 → 11792 kB | yes |
| tree's follower | 0.03 | 0.05 | 9368 → 9368 kB | yes |
| **sum** | **0.86** | | **57.4 MB** | |

(The first 62 s window gave 0.34 / 0.18 / 0.32 / 0.27 / 0.05 %; lifetime averages from the cumulative ticks are
0.2-0.3 %.) Memory is flat: no growth in 17 minutes, `VmHWM` equals `VmRSS`.

**2. How often each view reads the store and redraws while idle** (from the code, confirmed by `/proc/<pid>/io`;
`strace -c -p <live pid>` is **not permitted**: `strace: attach: ptrace(PTRACE_SEIZE, 1557240): Operation not
permitted`, `ptrace_scope` = 1, so I used the kernel's per-process counters instead):

| view | redraw while idle | store read while idle | measured on the live process (5 windows, median) |
|---|---|---|---|
| tree | every `TICK` = 100 ms (`tree/mod.rs:51,103`) = 10 Hz | none; the follower child polls (`events.poll_ms`); a snapshot ≥ 500 ms after a followed Event | 9.95 `write`s/s, 249 B/s; 0 `pread`/s |
| plan | every `poll` = `events.poll_ms` = 200 ms (`plan/mod.rs:196-197`) = 5 Hz | `Selection::fetch(cursor, …, 500)` each 200 ms (5 Hz), snapshot ≥ 500 ms after a concerning Event | 4.99 `write`s/s, 125 B/s; 0 `pread`/s |
| widgets | every `TICK` = 200 ms = 5 Hz (`widgets/mod.rs:34,71`) | `highest_id` every tick once 500 ms have passed since the last read (about 4 Hz), the **full snapshot every 2 s** | 4.98 `write`s/s; **945 KB/s, 238 `pread`/s** |
| statusbar | every 200 ms = 5 Hz (`statusbar/mod.rs:42,111`) | same as widgets | 4.98 `write`s/s; **945 KB/s, 236 `pread`/s** |
| worker | every 100 ms (`worker/mod.rs:43,112`) | `read::worker` + file `stat` every 500 ms regardless of Events (§17 says Event-driven) | not live (no Worker pane open) |
| logs | every `poll` = 200 ms | `highest_id` each 200 ms, appends only new Events | not live (popup) |
| history, chat, legend | only on input (`event::read()` blocks) | none | n/a |

**3. Cost of one snapshot per view.** In-process on the copy (`read::*` called from a scratch crate outside the
worktree, N = 200 each after one warm-up; ms):

| read | median | max | `pread`s per call |
|---|---|---|---|
| `highest_id` (one tick of widgets/statusbar/logs) | 0.009 | 1.831 | 0 |
| `tree()` (tree snapshot) | 2.070 | 3.833 | 579 (2.37 MB) |
| `widgets(6, 0)` (**widgets and statusbar**) | 1.542 | 1.711 | 466 (1.91 MB) |
| `chat_slot()` + `plan_boxes(pi-pin)` (plan snapshot; the chart layout is not included: private) | 0.097 + 0.312 | 0.993 + 0.391 | 15.5 (55 KB) |
| `logs(500)` | 1.002 | 2.243 | n/m |
| `logs_after(cursor)`, idle | 0.033 | 0.317 | 0 |
| plan view's idle `fetch(cursor, 500)` | 0.025 | 0.033 | 0 |
| same with 100 new Events | 0.220 | 0.263 | n/m |
| `worker(pi-pin, contract-a1)` | ≤0.17 (1000 calls in 0.17 s, process start included: a mean, not a median) | | 0.2 (0.8 KB) |

Through the equivalent read **command** (release binary, `CPD_HOME` = the copy, N = 20 each, process start
included; ms):

| view | command | median | max |
|---|---|---|---|
| (baseline, no snapshot) | `cpd config` / `cpd tiers` | 3.11 / 5.63 | 3.61 / 7.86 |
| tree | `context list` + `session list` | 6.27 + 10.74 | 6.74 + 11.53 |
| plan | `plan show --plan pi-pin` | 14.68 | 16.35 |
| widgets, statusbar | `plan list` | 10.76 | 12.05 |
| logs | `events list` | 8.60 | 9.39 |
| worker | `attempt show contract-a1 --plan pi-pin` | 5.32 | 5.99 |

Re-rendering a Worker's session file when it changes (`worker/mod.rs:129-170`): the largest file, 2.27 MB, takes
0.5 ms to read, 4.3 ms to render and 4.1 ms to wrap at 100 columns (N = 10; max 5.1 / 4.4 ms), about 9 ms at ≤ 2 Hz
while the file grows: no finding.

Not measured: the plan view's chart layout and the views' draw time (private to the crate; the CPU table includes
them), and macOS (`sysctl` per 2 s, W10): no macOS host.

### tui-F1 — widgets and statusbar re-read the whole Dispatch model every 2 s

- **what:** Both views call `read::widgets`, which computes `slots`, `engine`, `plan_cards`, `open_prs`,
  `escalations` and `recent_events`. The Deploy column draws **only** `prs` (+ the machine); the StatusBar draws
  **only** `slots`. Nothing reads `plans`, `engine`, `escalations` or `events` outside test fixtures. `plan_cards`
  runs `plan_heads`, a scan of the 1.5 MB `jobs` table, which is nearly all of the cost; both views do it every 2 s
  (`REFRESH`) even when no Event moved.
- **where:** `tui/src/read/widgets.rs:138-150`; `tui/src/widgets/mod.rs:49,82`; `tui/src/statusbar/mod.rs:80,136`;
  `tui/src/widgets/column.rs:122,125` (`w.prs` is the only field drawn); `tui/src/statusbar/workers.rs:22` (`slots`).
- **evidence:** live views, 5 windows of 60 s: widgets 945.10 KB/s and 237.8 `pread`/s, statusbar 945.18 KB/s and
  236.3 `pread`/s, while the tree (0.00) and plan (0.00) read nothing in the same minutes. In-process: one
  `widgets()` call = 1,906,155 B and 465 `pread`s (`/proc/self/io` before/after 100 calls); 945 KB/s × 2 s ≈ 1.9 MB
  confirms it is the whole snapshot per 2 s. Attribution by `strace -f -c -e trace=pread64 sqlite3 copy.db "<query>"`
  (3 `pread`s for `select 1`): `plan_heads` jobs query 541, `open_prs` 89, `labels` 79, `slots` 25, `engine` 28.
  `EXPLAIN QUERY PLAN` of `plan_heads`: `SCAN j USING INDEX sqlite_autoindex_jobs_1` + a correlated subquery per row.
- **impact:** About 1.9 MB/s and 474 `pread`s/s of avoidable file reads for the pair, forever, on a machine that also
  runs six Workers; about 0.1 % CPU each (inference: they run 0.20-0.25 % against the plan view's 0.13 % at the same 5 Hz draw rate; the rest is the draw loop, F2). In absolute terms
  small today; it scales with the `jobs` table (190 rows now) and gets worse with tui-dots (F3).
- **proposed fix:** Split `read::widgets` into `read::slots` (statusbar: 25 `pread`s) and `read::open_prs` (widgets:
  89), drop the unread fields; keep the 2 s refresh only where a source has no Event (pull requests, load).
  Expected: statusbar -95 %, widgets -80 % of their reads, measured the same way.
- **size:** Job.
- **overlap:** SQL itself (the `jobs` scan, the `labels`/`open_prs` subqueries) is Job `store-sql`'s. tui-visuals
  owns the Deploy column's drawing, not its read.

### tui-F2 — Every view redraws on every tick, and ratatui writes 25 bytes each time

- **what:** Each loop calls `terminal.draw` at the top of every iteration whether or not state changed. ratatui 0.30.2
  emits `ESC[39m ESC[49m ESC[59m ESC[0m ESC[?25l` on every `draw` even with an empty diff, so an idle pane writes to
  tmux 5-10 times a second.
- **where:** `tui/src/tree/mod.rs:103`, `plan/mod.rs:196`, `widgets/mod.rs:71`, `statusbar/mod.rs:111`,
  `worker/mod.rs:112`, `logs/mod.rs:82`.
- **evidence:** live: tree 9.95 `write`s/s and 248.8 B/s, plan 4.99/s and 124.9 B/s, widgets 4.98/s and 131.1 B/s,
  statusbar 4.98/s and 124.5 B/s (5 windows, median): 25 B per write, 24.9 writes/s for the four views. Cause:
  a scratch program with `ratatui::init()` and five identical `draw`s under `script -qec`, `strace -e trace=write`:
  `write(1, "\33[39m\33[49m\33[59m\33[0m\33[?25l", 25) = 25` once per draw.
- **impact:** Small: 0.13-0.25 % CPU per view in total, with the draw's share not separable (**unverified**); tmux
  parses about 630 B/s it does not need. tui-dots makes the tree and plan redraw every 250 ms for the spinner whether or
  not anything spins.
- **proposed fix:** Draw when a state flag is set (input, new snapshot, resize, clock second/spinner frame *while
  something is shown that depends on it*). Keep the tick only for polling.
- **size:** Job (with G2's shared loop).
- **overlap:** tui-dots (`FRAME` = 250 ms in `tree/mod.rs` and `plan/mod.rs`) decides the spinner's redraw; do this
  after it, keyed on "a spinner is on screen".

### tui-F3 — One tree snapshot is 2.4 MB; tui-dots will take it every 2 s

- **what:** `read::tree` costs 2.07 ms and 2.37 MB (579 `pread`s) per call. Today the tree takes it only on a
  followed Event (about 7 followed Events per busy minute in the copy; at most 48 in one minute): the live tree read
  19.6 MB in the 62 s window that had Events and 0 in the 5 quiet windows. tui-dots' `SNAPSHOT_AT_LEAST_EVERY` = 2 s
  (`origin/cpd/tui-dots/cpd:tui/src/tree/mod.rs`) makes it a steady 2.37 MB per 2 s.
- **where:** `tui/src/read/tree.rs`, `tui/src/read/shared.rs` (`plan_heads`); `origin/cpd/tui-dots/cpd`.
- **evidence:** bench: `tree n=100: rchar 237318868, syscr 57952` against `n=0: 336596, 95` → 2,369,823 B and 578
  `pread`s per call. Events in the copy: `select count(*) …` 468 minutes with followed kinds, median 7, max 48.
- **impact:** **Projection, not a measurement:** +1.2 MB/s and +290 `pread`s/s for the tree once tui-dots merges,
  on top of F1's 1.9 MB/s, about 3.1 MB/s for the three views.
- **proposed fix:** Fix F1 and the `plan_heads` scan (store-sql) before or with tui-dots' 2 s re-read; re-measure with
  the commands in Method.
- **size:** Plan (spans Jobs of store-sql and tui).
- **overlap:** Plan tui-dots (the 2 s re-read); Job `store-sql` (the query).

### tui-F4 — The tree's follower is a whole `cpd` process

- **what:** One `cpd events follow` child (its own SQLite connection, 9.4 MB RSS) and a reader thread exist only to
  tell the tree "an Event came". The plan, widgets, statusbar and logs views do the same in-process (§17 allows both).
- **where:** `tui/src/tree/follow.rs:55-78`.
- **evidence:** `ps`: tree 12292 kB + follower 9368 kB (+76 %) against plan 11660 kB; follower CPU 0.03 % median.
- **impact:** 9.4 MB and a process per cockpit; the real cost is W2 (spawn path, silent failure, teardown).
- **proposed fix:** G4: the in-process poll, deleting `follow.rs`'s process and thread.
- **size:** Job (inside G4).
- **overlap:** none.

---

## Good

### tui-G1 — The scroll/wrap code exists three times
- **what:** `worker/view.rs` and `history/view.rs` share about 75 lines (the key handling, the wheel, `scroll_by`, the
  wrapped-lines cache); `logs/view.rs` has its own `scroll_up/scroll_down/page`.
- **where:** `tui/src/worker/view.rs:30-100`, `tui/src/history/view.rs:30-100`, `tui/src/logs/view.rs:51-65`.
- **evidence:** `diff <(sed 's/history/X/g;s/worker/X/g' tui/src/worker/view.rs) <(sed 's/history/X/g;s/worker/X/g'
  tui/src/history/view.rs)` differs in the header line, the status/foot, the top line and the tests only; `diff
  worker/view.rs history/view.rs | grep -c '^[<>]'` → 94 of about 190 lines.
- **impact:** A fix to one (the `q` rule of W1, say) has to be made in two or three places.
- **proposed fix:** One `Scroller` (lines, wrap cache, follow-the-end, keys, wheel); each view adds its header/foot.
- **size:** Job.
- **overlap:** none.

### tui-G2 — Terminal set-up and failure handling are copied into nine views
- **what:** `ratatui::init()` / `EnableMouseCapture` / `restore()` appear nine times, `terminal_failed` is defined
  three times (two closures and a fn) and inlined as `map_err` in the other views, and `ErrorCode::NotATerminal` is used for any
  terminal I/O failure ("The terminal of cpd tui view X failed"), not only for "stdin is not a terminal". The
  widgets and statusbar loops are the same loop (`TICK`/`COALESCE`/`REFRESH` constants declared twice). This is where
  W1, W4, W9 and F2 would each be fixed once.
- **where:** `grep -rn "ratatui::init()" tui/src | wc -l` → 9; `grep -rn "terminal_failed" tui/src`;
  `tui/src/widgets/mod.rs:34-38` and `tui/src/statusbar/mod.rs:42-46`.
- **impact:** The cause of the inconsistencies in W1/W4; any new view copies the boilerplate.
- **proposed fix:** `fn run_view(name, Setup, impl FnMut(&mut Terminal) -> Result<()>)` owning init, mouse/focus
  capture, a `Drop` guard that disables them (W9), error mapping, and the retry/note policy (W4); a `Refresher`
  (coalesce + at-least-every) for widgets/statusbar/plan.
- **size:** Plan (touches all views) or Job for the runner plus one Job per view.
- **overlap:** tui-dots and tui-visuals edit the same loops: sequence after them.

### tui-G3 — Dead code
- **what:** (a) `plan::Actions::run_command` and `Actions::open_lead` are never used (the build warns), so the
  `RunCommand` parameter threaded through `plan::run` serves nothing. (b) `read::flow()`, `Flow`, `Review`,
  `open_escalations`, `latest_review` are used by no view since R34 (the plan view reads `plan_boxes`), only by
  `cli/tests/tui_read.rs` (`:465-533`). (c) `Widgets.{plans, engine, escalations, events}` are read and never drawn
  (F1).
- **where:** `tui/src/plan/actions.rs:30,44`; `tui/src/read/flow.rs:169-176`, `tui/src/read/mod.rs:34`;
  `tui/src/read/widgets.rs:24-36`.
- **evidence:** `scripts/cargo.sh build --release -p cpd` →
  `warning: field `run_command` is never read --> tui/src/plan/actions.rs:30:5` and
  `warning: method `open_lead` is never used --> tui/src/plan/actions.rs:44:12`.
  `grep -rn "read::flow\|flow(&mut" tui/src cli/src` → none outside `read/`; the branches of tui-dots, manager and
  tui-visuals do not use it either (`git grep` on each).
- **impact:** Code and tests to maintain that nothing runs, plus a read path (`latest_review`) that looks live.
- **proposed fix:** Delete (a), (b) with their tests, (c) with F1. Check the fence first: (b) is public API of
  `cpd_tui`; the only caller is the test file.
- **size:** Job.
- **overlap:** build warnings listed in other areas' findings (`core/src/plan/row.rs`) belong to their Jobs.

### tui-G4 — Four ways of following Events
- **what:** tree: subprocess + reader thread + channel; plan: in-process `Selection::fetch` of full rows (500) every
  `poll_ms`; widgets, statusbar, logs: `highest_id` + re-read; worker: a store read every 500 ms with no Event at all.
  Each with its own coalescing code. §17 says "through `cpd events follow` or the same poll in process".
- **where:** `tree/follow.rs`, `plan/mod.rs:197-208`, `widgets/mod.rs:79-86`, `statusbar/mod.rs:133-140`,
  `logs/mod.rs:101-104`, `worker/mod.rs:98-110`.
- **impact:** Each variant has its own failure behaviour (W2, W4); none is shared or tested once.
- **proposed fix:** One `Follow` in `read/` (`poll(store) -> Result<Vec<Event>>`, cursor kept, `concerns` filter,
  coalescing), used by all; delete the tree's follower (also F4, W2).
- **size:** Plan.
- **overlap:** tui-dots changes the tree's snapshot schedule; hs-d140 only the `events follow` command.

### tui-G5 — Docs that no longer match
- **what:** (a) `logs/mod.rs:1-3,42` call the button "` ≡ logs `"; it is `▤` (`statusbar/mod.rs:51`, commands.md §13.2).
  (b) `plan/mod.rs:14`: "a layout pane that dies stays dead (contract §6.1)": the hook respawns it (W1).
  (c) commands.md §17 says views apply `session-state`/`session-renamed`/`session-moved` data directly, and the
  worker view follows `session-state`/`attempt-status`; the tree always takes a full snapshot (F3) and the worker
  polls the store every 500 ms (`worker/mod.rs:98-110`). Either fix is fine (the poll costs ≈0.14 ms); the text
  should say which.
  (d) `tui/src/tree/mod.rs:14-15` points to `tui/README.md` "on `main`"; `ls tui` → `Cargo.toml src`, so it is the cpd 1 spike's file (a pointer, not a defect).
- **evidence:** `grep -n "≡" tui/src -r` → `logs/mod.rs:2,42`; `ls tui` → `Cargo.toml src`.
- **proposed fix:** Edit the comments and §17 together with W1/G4.
- **size:** Job.
- **overlap:** none.

---

## Already good, keep

- **The terminal is restored on every error path** (not on panic: W9): `ratatui::restore()` runs after the loop in all
  nine views whether it returned `Ok` or `Err`, and the terminal error becomes a `CpdError`
  (`tree/mod.rs:63-72`, `plan/mod.rs:166-176`, `statusbar/mod.rs:82-92`).
- **No `unwrap`/`expect`/unchecked index on read data in the production paths of the views.** The grep for
  `unwrap()|expect(|panic!|unreachable!` outside `#[cfg(test)]` finds only static invariants
  (`rows.rs:331,357` unique ids, `follow.rs:65` piped stdout, `history/view.rs:84`, `worker/view.rs:83`
  "wrapped above", `cockpit.rs:220`). `rows.rs:331`'s "Plan and Session ids are unique" holds because Plan ids are
  slugs and Session ids UUIDs; I did not prove that a Session cannot appear twice under one Context (unverified).
- **Dependency cycles in PR or Job needs cannot hang or overflow the layout**: `boxes::columns` and
  `flow::column_of` carry an `open` set and `reduced_needs` a visited set (`plan/boxes.rs:36-55`, `plan/flow.rs:140-197`).
- **Teardown of the follower:** `Drop for Follower` kills and waits; an orphaned `events follow` ends by itself when its
  reader is gone (`core/src/events/mod.rs:182`, hardening G1). `ps` on the live machine shows exactly one
  `events follow --kind …` process, the live tree's child, and none orphaned.
- **Gap-free start is right:** the plan snapshot reads the cursor first (`plan/mod.rs:64-65`); every read model
  returns `events_cursor` from its own transaction; reads take no lock (an idle `highest_id` is 0.009 ms and zero
  `pread`s).
- **The coalescing rules of §17 are implemented:** 500 ms between snapshots in tree, plan, widgets, statusbar; the
  2 s refresh for the sources with no Event; `events.poll_ms` honoured by plan and logs.
- **Snapshots are cheap and memory is flat:** 0.3-2.1 ms in-process per snapshot, 11.6-12.3 MB per view, no growth
  in 17 minutes; re-rendering a 2.3 MB session file is about 9 ms.
- **Tests:** 46 `#[test]`s in `tree/view.rs` (keys, prompts, menus, clamping, margins), unit tests for slug,
  rows, boxes, layout, statusbar, and tmux-driven tests per view in `cli/tests/tui*.rs` (the hook-less setup aside, W1).
- **The history view's `q` already avoids the respawn** by showing the placeholder in the same process
  (`history/mod.rs:88-91`): the pattern to copy in W1.
- **Actions are commands, never writes:** the tree and plan views only run `cpd <cmd>` (`Action::argv`), with typed
  text passed as `--flag=value` or after `--`.

---

## Method

Environment: Linux, tmux 3.4, `CLK_TCK` = 100, `/proc/sys/kernel/yama/ptrace_scope` = 1, load average 12-17 during the
work (other Workers building). Branch `cpd/quality/tui` at `5ef69b2` + this file. No timing was taken on `~/.cpd`,
the live engine or the `-L cpd` server; the live view processes were only read.

**The copy** (hs-d157): `T=$(mktemp -d /tmp/tui-a1-home.XXXXXX); sqlite3 ~/.cpd/cpd.db ".backup $T/cpd.db"; cp
~/.cpd/config.toml $T/; cp -r ~/.cpd/contexts $T/`, then `printf '\n[tmux]\nsocket = "quality-%s"\n' $$ >> $T/config.toml`
(the config had no `[tmux]` table). `sqlite3 $T/cpd.db "select count(*) …"`: events 4689, sessions 201, attempts 190,
jobs 190, prs 26, pull_requests 24, plans 20; `pragma user_version` 6; `dbstat`: `history` 21 MB, `jobs` 1.5 MB,
`events` 1.1 MB. Binary: `scripts/cargo.sh build --release -p cpd` → `target/release/cpd`. Every command below ran
with `CPD_HOME=$T`. The copy and every temporary file were deleted at the end.

**Commands run, in order of use**
- Reading: `cpd context show --context building-cpd` (live, read-only, for the Context records); `git log/diff
  origin/main..origin/cpd/<plan>/<key> --stat` and `git grep … origin/cpd/<branch>` for tui-dots, manager, tui-visuals,
  pi-pin, dev-speed; `grep`, `diff`, `sed -n`.
- Live views, read-only: `ps -eo pid,ppid,etime,pcpu,rss,args | grep -E 'tui view|events follow'`; `/proc/<pid>/stat`
  (fields 14+15), `/proc/<pid>/io` (`rchar wchar syscr syscw`), `/proc/<pid>/status` (`VmRSS VmHWM`, context
  switches), `ls -l /proc/<pid>/fd`: a first sample, a second 62 s later, then `/tmp/tui-a1-sampler.sh` (6 samples
  60 s apart = N 5 windows). `timeout 12 strace -c -f -p 1557240` → `Operation not permitted`.
- In-process snapshot timing: a scratch crate in `/tmp/tui-a1-bench` (outside the worktree; path dependencies on
  `core/` and `tui/`, own `CARGO_TARGET_DIR`, `scripts/cargo.sh build --release --offline`) calling `read::tree`,
  `widgets(6,0)`, `chat_slot`, `plan_boxes`, `logs`, `logs_after`, `worker`, `Selection::fetch`, `highest_id` N = 200
  times with `Instant`; `/proc/self/io` before/after 100 calls for `rchar`/`syscr`; a second binary that includes
  `tui/src/worker/render.rs` and `palette.rs` with `#[path]` to time `render` and `wrap` on the two biggest session
  files (N = 10); a third with `ratatui::init()` and five `draw`s run as `strace -f -e trace=write -o … script -qec
  ./draw /dev/null`.
- Per-query I/O: `strace -f -c -e trace=pread64 sqlite3 $T/cpd.db "<query>"` for the `plan_heads` jobs query, `open_prs`,
  `labels`, `slots`, `engine`; `sqlite3 $T/cpd.db "EXPLAIN QUERY PLAN …"`.
- Commands: `for i in 1..20: date +%s%N; cpd <cmd> >/dev/null; date +%s%N` for `config`, `tiers`, `context list`,
  `session list`, `plan show --plan pi-pin`, `plan list`, `events list`, `attempt show contract-a1 --plan pi-pin`;
  median and max of 20.
- tmux: a private server `tmux -L quality-hook-$$` (hook demonstration, W1) and `tmux -L quality-lat-$$` (20 ×
  `list-panes -a`); each ended with `kill-server`, `tmux -L … ls` → `no server running`. Also `cp /bin/sleep /tmp/tui-a1-x;
  /tmp/tui-a1-x 20 & rm …; readlink /proc/$!/exe` (W2).
- Not run (out of bounds): `cpd tui`, `tui view …`, `watch`, any write command; hence W2's repeated spawns, W4's retry
  and W5's missing pane are read from code and marked unverified.
- Quick check: `npm run typecheck` and `scripts/cargo.sh build --workspace` from the worktree.

**Not taken:** `strace -c` on a live view (not permitted: `/proc/<pid>/io` was used); macOS `sysctl` cost; the plan
chart layout and the draw time in isolation; the duration of a blocking action (W7).
