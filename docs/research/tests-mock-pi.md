# tests-mock: inventory of pi/test, pi/skills, pi/compact-tools, the scripts and the Gate wiring (Job inv-pi, a1)

Static analysis only (`rg`, `sed`; nothing run but `nextest --version` and strings of the nextest binary). The rule: tests test only our code;
tmux, pi, gh, curl, ssh, plannotator, the network and real waiting are mocked; allowed: files in the test's temp dir, our own shims
(processes and signals they start, reaped), node as the runner. `scripts/pi-check.sh` (real pi) stays the named Gate exception.
Test counts are `rg -c '^\s*test(\.\w+)?\('` lines (a `for` loop around `test(` counts once): approximate, `unverified` against `node --test`.
Base: `HEAD` carries the other `tests-mock-*.md` inventories; the local `origin/main` ref is older, so `git diff --stat origin/main`
also lists them. This Job's own diff is this one file.

## Bottom line

- **No TypeScript test uses real tmux, git, gh, curl, ssh, the network, a port or a real `pi`, except `pi/test/pi-check.test.ts`** (real tmux in 8 of its 11 tests, and the real pinned pi in one). `rg -l "node:(net|http|https|dgram|dns|tls|worker_threads)" pi` finds nothing.
- **Real git: only `pi/test/mainguard.test.ts`** (17 tests), and it does not need it: `pi/lib/mainguard.ts:24-36` only `lstat`s `.git` (a directory = primary checkout, a file = linked worktree). A `mkdir .git` / `writeFile .git` fixture replaces `git init/commit/worktree add`.
- **Real processes the rule allows**: `pi/test/fake.ts` writes a node script as the fake `cpd` (`fake.ts:12-72`) and as the fake pi (`fake.ts:79-93`), runs each `cpd` call as a real child (`FakePi.exec`, `fake.ts:386-396`, `execFile`) and the follower as a real child (`fake.ts:245-258`). Each shim is SIGTERMed and its pid waited for in `Fixture.close()` (`fake.ts:287-306`). Allowed, but every Fixture test then waits in real time (below).
- **Real waiting is the TypeScript suite's main violation**: `waitFor` polls with a real `sleep(10)` and a 15 s wall-clock deadline (`fake.ts:570-577`); about 330 tests in 21 files use `Fixture`, and about 320 `waitFor(` call sites sit in 21 files. The tests also wait real adapter timings (30-300 ms) and shim delays (100 ms - 10 s).
- **`scripts/pi-check.sh` can run without tmux** (it needs a tty, not tmux): cheapest a python3 `pty` wrapper, about 25 lines (section 2.2). Real pi stays; real tmux goes.
- **`pi-check.test.ts` is invisible to the Gate's leak check**: every run uses `env` = `baseEnv()` (`pi-check.test.ts:37`) with no `GATE_RUN`, so its tmux socket is `pi-check-x-<pid>` (`pi-check.sh:55`), which `scripts/gate-run.sh:108` (`*-$GATE_RUN-*`) never matches, and its processes carry no `GATE_RUN` for the scan (`gate-run.sh:68-77`). Its dirs are under `/tmp` (`pi-check.test.ts:14`), not `$TMPDIR` (`gate-run.sh:116`).

## 1. Summary table

Columns: file | tests | real tmux | real git | own processes/signals | sleep or poll | other external | class.
`W` = `waitFor` (real 10 ms poll, `fake.ts:570-577`); `F` = `Fixture` (fake node `cpd` per call; own processes, reaped in `close()`).

### pi/test, top level (48 test files, about 511 tests)

| file | tests | tmux | git | own processes / signals | sleep or poll | other | class |
|---|---|---|---|---|---|---|---|
| pi-check.test.ts | 11 (9 `test(` lines) | **real, through `scripts/pi-check.sh` `tmx`** (`pi-check.sh:82`): :39, :81, :110, :240, :255, :292, :322 x2 (8 tests); :275 only `kill-server` of no server; :178 x2 step 0 never reaches tmux | none | spawnSync of the script (`:35`, 80 s timeout); real pinned pi in :81 (`node_modules/.bin/pi`, `:85`); a background `date`-loop writer for 3 s (`:300-301`) | `spawnSync("sleep","0.5")` (`:315`); the script's own `sleep 0.1/0.5` (`pi-check.sh:101,129,182`); `Date.now` bound `<30 s` (`:260-269`) | real `pi` (:81), `tmux` | **T** (8), **X** (1: :81 tests pi) , K (2 step-0) |
| mainguard.test.ts | 21 | none | **real**: `git init -q -b main`, `commit --allow-empty`, `worktree add -b` x2 (`:26-45`), in 17 tests (`rg -c "fixtures\(t\)"`) | F in the 4 adapter tests (`:196-238`) | W (1 site) | none | **G** (17; fix: no git, `.git` dir/file) , K (4 skills/pure) |
| follower.test.ts | 9 | none | none | F; `spawn("cpd-does-not-exist")` (`:153-156`) real failed spawn; SIGTERM + `killed-<pid>` (`:166-176`) | W x20; real `setTimeout(run, ms)` backoff (`:34,72`); retries 30-120 ms | none | **C** |
| reload.test.ts | 25 | none | none | F (5); node fake pi per validation (`fake.ts:79`); `HANGS` extension never settles, killed by SIGKILL of its group after 300 ms (`:314-332`, `reload.ts:100-117`) | W x34; real 300 ms load timeout (`:319`); real `Date.now() - from > 100` (`:510-513`); `t.mock.timers` only for `setInterval` (`:212,251,391`) | none | **C** (4), K (rest) |
| start.test.ts | 16 | none | none | F; a shim `delay: 10_000` aborted by `callTimeoutMs: 300` / SIGTERM (`:219-259`) | W x16; `Date.now() < 5000` bounds (`:221-223,255-257`) | none | **C** (2: :215, :246), K |
| state.test.ts | 16 | none | none | F; shim `delay` 120 / 3000 ms (`:37,115,216`); `shutdownWaitMs: 200` (`fake.ts:250`) | W x31 | none | **C** (4) |
| delivery.test.ts | 21 | none | none | F; shim `delay: 100` (`:264`) | W x27 | none | **C** (1), K |
| plan-review.test.ts | 34 | none | none | F | W x47 | none | **C** (the W only) |
| review-chat.test.ts | 33 | none | none | F | W x45 | none | **C** (the W only) |
| loop-guard.test.ts | 12 | none | none | F | W x24 | none | **C** (the W only) |
| askguard.test.ts | 26 | none | none | F | W x1 | none | K (F) |
| updates.test.ts | 30 | none | none | F | W x1 | none | K (F) |
| reply, retier, handle, first-message, protocol, manager-errors, policy | 7, 8, 5, 6, 10, 8, 8 | none | none | F | W: 9, 8, 9, 5, 6, 3, 0 | none | K (F), **C** for the W |
| reminder, noblock, testguard | 12, 11, 8 | none | none | F in a few | W x1 each | none (the `sleep 30` / `npm test` strings are inputs to a parser, `noblock.test.ts:11-27`, `testguard.test.ts:6-60`) | K |
| slack-channel.test.ts | 10 | none | none | `spawnSync(node, helper)` (`:145`, 30 s timeout) | `setTimeout(r, 2)` x30 (`:99`) | fake MCP at `https://fake.invalid/mcp` (a string, no connection, `:122`) | K (real node child, allowed), tiny **C** (`:99`) |
| answers.test.ts | 5 | none | none | `execFileSync(node, answers.ts)` (`:62`, 20 s timeout) | none | none | K |
| gate.test.ts | 1 | none | none | none | none | reads `package.json` (`:10-24`) | K (pins `verify:steps` verbatim) |
| no-fixed-sleeps.test.ts | 1 | none | none | none | none | the pi tests' own sleep guard (`:7-15`), only `pi/test/*.ts` and only `sleep(` / `settle(` | K |
| render, chat-pad, state-less pure files: answers-free (balloon, balloon-margins, ask-header, ask-tabs, event, folds, hint, icons, mentions, mentions-refresh, pill, pill-brand, pill-split, pi-theme, pi-types, pnguard, pntrack, rail-margins, slack-parse, structure) | about 150 | none | none | none (render.test.ts and chat-pad.test.ts hold one `new Fixture` each) | none | none (pi-theme, structure, protocol read repo files) | K |

### pi/test/compact-tools (59 test files, about 452 tests) and pi/skills

| file | tests | tmux / git | own processes | sleep or poll | other | class |
|---|---|---|---|---|---|---|
| compact-tools.test.ts | 69 | none | none | **real `setTimeout(270)` x3** (`:705,712,718`) on the real `ToolRuntime` interval (`compact-tools-runtime.ts`); fake timers elsewhere (`:1561-1562`) | none | **C** (1 test: "stops the shared indicator timer...", `:700-720`), K |
| editor-box.test.ts | 12 | none | none | **real `setTimeout(100)`** (`:209`) waiting for an async autocomplete | none | **C** (1) |
| animation-clock, live-off-public, live-switch, off-native-ui, render-golden, ui-lifecycle, indicator-harness | 3, 2, 5, 1, 5, 4, - | none | none | `@sinonjs/fake-timers` `install({toFake:["setInterval","clearInterval"]})` and `Date.now` replaced (`render-golden.test.ts:72-75`, `indicator-harness.ts:24-33`); `sleep 1` only inside command strings (`live-switch.test.ts:58-69`, `codex-ui.test.ts:244`) | none | K |
| other 51 files | about 420 | none | none | none | read fixtures/goldens; `https://...` only inside strings (`render-golden.test.ts:291`, `thinking-plain.test.ts:72`) | K |
| skills/skill.test.ts | 35 | none | none | none | reads `docs/*.md`, `core/src/spec/worker.md`, `pi/lib/*.ts` (`:6,269-270,540-542,702-707`); `git`, `gh`, `tmux` appear as text the skills may name (`:124-132,390-391,514-517,585-606`) | K (a doc-vs-code test: couples to docs, not an external) |

### scripts and Gate wiring

| file | tests | tmux | git | own processes | sleep or poll | other | class |
|---|---|---|---|---|---|---|---|
| scripts/pi-check.sh | (tested by `pi-check.test.ts` + `cli/tests/tooling/pi_check.rs`, 4 tests, PATH-shim tmux, `pi_check.rs:20,62`) | real `tmux -u -L` (`:82`): `new-session` (`:164`), `has-session` (`:173`), `capture-pane` (`:174`), `list-panes` (`:86`), `kill-server` (`:114`) | none | the real pinned pi in a pane, a fake `cpd` sh script (`:152-159`), pane process-group kill (`:117`) | `sleep 0.1` x2 (`:101,182`), `sleep 0.5` (`:129`), `exec sleep 60` in the fake cpd (`:156`) | real pi (`:138,142,164`), node | **T** (tmux), the pi stays X-exception |
| scripts/gate-run.sh | `cli/tests/tooling/gate_run.rs` (8, see tests-mock-tooling.md:31) | scans `/tmp/tmux-<uid>` sockets (`:36,107-114`) | none | `ps -axww -E` / `/proc` scan (`:68-78`), `kill -KILL` (`:139`) | none | none | K (the wiring) |
| scripts/check-portable.sh | `cli/tests/tooling/install_script.rs` lints (tests-mock-tooling.md:155) | static (`bare_tmux`, `:104-151`) | none | none | none | none | K |
| scripts/install-cpd.sh, preview-tui.sh | `install_script.rs`, `preview_script.rs` | real `tmux -u -L` (`install-cpd.sh:34`, `preview-tui.sh:24`) | git | none | `sleep 0.25` (`install-cpd.sh:268`) | none | not this Job (tests-mock-tooling.md) |
| scripts/{check-migrations-frozen,npm-fresh,repin,sync-version,tag-version,new-migration,cargo,tests,test-changed}.sh | `*_script.rs` | none | git in some | none | none | none | not this Job |
| .config/nextest.toml | - | `tmux-restart` group (`:71-80`) for `cpd::tui bare::` and an install test | - | `leak-timeout` 100 ms, result fail (`:24`); `slow-timeout` 30 s x3 (`:33`) | - | - | K |
| package.json `verify:steps` (`:15`) | `pi/test/gate.test.ts` | - | `git rev-parse` for `CPD_COMMIT` | - | - | - | K |

## 2. Clusters

Size unit: one Worker Job, about 150 turns, at most about 40 tests rewritten.

### 2.1 Clusters of tests

| # | cluster | count | pattern to list it | external touched | fix and seam | Jobs |
|---|---|---|---|---|---|---|
| A | `pi-check.test.ts` runs the script with **real tmux** (and real pi in one test) | 8 tests (of 11), in 7 `test(` lines | `rg -n "run\(.*pi-check\|passingPi\|copyCheckout" pi/test/pi-check.test.ts`; first `:39`, last `:322` | tmux subcommands the script calls: `-u -L <s> -f /dev/null new-session -d -x 120 -y 30 -c <dir> <cmd>`, `has-session`, `capture-pane -p`, `list-panes -a -F #{pane_pid}`, `kill-server` (`pi-check.sh:86,114,164,173,174`); real pi: `--version`, rpc load, interactive | Put a **fake `tmux` shell script first on PATH** (the script calls `tmux` by name through `cleanenv`, which keeps PATH, `pi-check.sh:67-82`): `new-session` runs its command in the background, `has-session` exits 0 while it lives, `list-panes` prints its pid, `kill-server` kills it. `cli/tests/tooling/pi_check.rs:20,62` already uses this seam (`env.fake("tmux", ...)`). Delete :81's real-pi run (the Gate step is that check, `package.json:15`) or keep its negative half as a unit test of `answers.ts` against `parseSession`; keep the `-u` and `env -i` tests on the fake | 1 |
| B | the same script waits **real time**: `sleep 0.1` x2, `sleep 0.5`, a 3 s writer, a 1 s `PI_CHECK_CLEANUP_SECS` | 4 tests (:255, :292, :240, :275) | `rg -n "sleep\|Date.now\|PI_CHECK_CLEANUP_SECS" pi/test/pi-check.test.ts` | `sleep` (shell) | A fake `sleep` on PATH (returns at once) next to the fake tmux; drop the `date`-loop writer (a fake `kill-server` that leaves a file for the cleanup to retry covers hs-d463 (d)) and the `Date.now()` bound. In Job A | in A |
| C | `mainguard.test.ts` runs **real git** to make a repo and worktrees | 17 tests | `rg -n "fixtures\(t\)\|git\(" pi/test/mainguard.test.ts`; first `:49`, last `:233` | `git init -q -b main`, `commit --allow-empty`, `worktree add -q -b <b> <dir>` x2, `execFileSync` (`:26,35-42`) | None needed: `primaryCheckoutMove` reads only `lstatSync(dir/.git)` (`pi/lib/mainguard.ts:24-36`). Fixture = `mkdir primary/.git`, a file `wt/.git` containing `gitdir: ...`. No fake git, no git contract test | 1 (small, can share a Job with E) |
| D | the **Fixture** itself: fake `cpd` is a node child per call plus a node follower polling files every 10 ms (`fake.ts:12-72`), `waitFor` polls real time (`fake.ts:570-577`), `quiet()`/`exited()`/`close()` wait on real pids (`fake.ts:260-306`) | about 330 tests in 21 files; about 320 `waitFor` sites | `rg -c "new Fixture\(" pi/test/*.test.ts`; `rg -c "waitFor\(" pi/test/*.test.ts` | node processes (allowed, our shims); real `Date.now`, `setTimeout` | The adapter already takes its boundaries as options: `Options.spawn`, `Options.schedule`, `Options.timing` (`fake.ts:245-258`) and `PiApi.exec` (`FakePi.exec`, `fake.ts:386`). Make `FakePi.exec` and `options.spawn` **in-process fakes** (an `EventEmitter` child with `stdout`, `kill()`, `exit`) answering from the same `resp/` map in memory, and make `waitFor` event-driven (resolve from the fake's own notification on each recorded call / sent message / exit, with a deadline only to fail a hang, not as a poll). Then no node child, no pid file, no `alive()`. `answers.ts` stays the single source (pi-check's fake still uses it) | 1 Job for `fake.ts` (582 lines) and `answers.ts`; then 3 Jobs to move the 21 files, each about 110 tests but mostly no edits beyond `waitFor` call sites; **4 total** |
| E | tests of real adapter timing: shim `delay` and short real limits | about 11 tests | `rg -n "delay:\|callTimeoutMs\|shutdownWaitMs\|timeoutMs" pi/test/*.test.ts`: `state.test.ts:37,115,216`, `delivery.test.ts:264`, `start.test.ts:219-220,250`, `reload.test.ts:319-332`, `follower.test.ts:28-43,72,153` | real limits of the product: `callTimeoutMs` 300 ms (test) / 30 s (default, `follower.ts:18`), `shutdownWaitMs` 200 ms / 2 s, restart back-off 30-120 ms / 1-30 s, reload load timeout 300 ms / `LOAD_TIMEOUT_MS`, `requestTimeoutMs` 300 ms | `Options.schedule` exists (`follower.test.ts:34`); add a clock seam to `startup.ts:226,260` (`setInterval`, `sleep`) and `reload.ts:109` (`setTimeout`) and use node's `t.mock.timers` / `@sinonjs/fake-timers` (already a devDependency, `package.json`; used in 9 compact-tools files and `reload.test.ts:212`). A "hang" is a fake child that never ends and a clock advance | 1 (after D) |
| F | real `setTimeout` waits in compact-tools tests | 2 tests | `rg -n "setTimeout" pi/test/compact-tools` = `compact-tools.test.ts:705,712,718`, `editor-box.test.ts:209` | `ToolRuntime` indicator `setInterval` (`compact-tools-runtime.ts`, `compact-tools-clock.ts:21`) | `install({toFake:["setInterval","clearInterval"]})` and `clock.tick(270)`, as `animation-clock.test.ts:7,29`; for `editor-box.test.ts:209` await the provider's promise instead of 100 ms | 1 (shared with E) |
| G | `slack-channel.test.ts` helper child and `setTimeout(2)` | 1 + 1 test | `slack-channel.test.ts:99,145` | node child, our helper, a fake pi tree in a temp dir (`:104-141`) | keep the child (our script, node, mocked MCP); replace `setTimeout(r, 2)` with a microtask yield | in E/F |

### 2.2 Question (2): can `pi-check.sh` drop tmux?

**What it needs a tty for.** `resolveAppMode(parsed, stdinIsTTY, stdoutIsTTY)` returns `"interactive"` only when both are ttys, else `"print"` (`node_modules/@earendil-works/pi-coding-agent/dist/main.js:81-91`, called with `process.stdin.isTTY, process.stdout.isTTY` at `:521`). The adapter returns unless `ctx.mode === "tui"` (`pi/lib/startup.ts:32`). pi-tui calls `process.stdin.setRawMode` (`pi-tui/dist/terminal.js:122-123`) and reads `process.stdout.columns/rows`, falling back to `COLUMNS`/`LINES` env then 80x24 (`:423-427`). So it needs a **pty with a size**, not a multiplexer. Nothing in `pi-check.sh` uses a tmux feature beyond: run a command in a tty, see whether it is alive, dump the screen on failure.

**Candidates**

| way | verdict |
|---|---|
| **python3 `pty`** (`pty.fork()` or `os.openpty()` + `subprocess.Popen(start_new_session=True)`, `fcntl.ioctl(TIOCSWINSZ)` 120x30, drain the master in a loop, `os.killpg` on exit) | **Cheapest.** python3 is already named as the portable alternative in `check-portable.sh:34-53` and the docs table; `/usr/bin/python3` exists here (verified with `which`). About 25 lines, in a `python3 - <<'PY'` block or `scripts/pi-check-pty.py` (a new file in `scripts/` must be added to the covered list, section 3.4) |
| `script` | **Not portable**: BSD (`script -q /dev/null cmd args`) and util-linux (`script -qec 'cmd' /dev/null`) differ; `/usr/bin/script` exists on this Mac (verified) but a Linux run is unverified. Rejected |
| `expect` | `/usr/bin/expect` exists here (verified); not a Linux base tool (unverified). Rejected: a new tool |
| node-pty | A native addon (node-gyp or prebuilds per platform), a new pinned dependency, a postinstall build; against the "take what upstream owns, configure nothing new" axiom for a 25-line job. Rejected |
| pi SDK in-process | pi exports `InteractiveMode` (`dist/modes/interactive/interactive-mode.js`); whether it accepts an injected terminal is `unverified` and would test pi's internals, not the shipped CLI. Not recommended |

**Cost.** Delete from `pi-check.sh`: step 0's tmux check (`:42-45`), the socket/`long_path` logic (`:55-63`), `TMUX_TMPDIR` in the allow list (`:75-80`), `tmx` (`:82`), `record_panes`, `group_alive`, `wait_panes` (`:84-104`) and their use in `cleanup` (`:113-119`), `new-session` (`:164-167`), `has-session`/`capture-pane` (`:173-174`): about 60 lines out, 25 in. Also delete the tests and doc text bound to tmux: `pi-check.test.ts:177-191` (step 0 tmux), `:321-341` (UTF-8 client), `cli/tests/tooling/pi_check.rs` (4 tests: socket under 100 characters, `TMUX_TMPDIR`), `check-portable.sh`'s `bare_tmux` row only for this file (still needed for `install-cpd.sh`, `preview-tui.sh`), `docs/implementing.md:343-361` (step 0, the socket-length rule, the UTF-8 rule), and `gate-run.sh`'s `pi-check-<id>` socket mention (`docs/implementing.md:139`). Then no real tmux remains in `pi-check.test.ts` and none in the script; **`gate-run.sh` still scans `/tmp/tmux-<uid>`** for the Rust tests (`:36-37,107-114`).

**Risks (all `unverified` until run once on this Mac and on hs).**
1. pi sends terminal capability queries (kitty keyboard `CSI ? u`, cell size, bracketed paste) that tmux answers or drops; on a bare pty nobody answers. Whether pi waits for the reply or times out is `unverified`; a timeout would only slow step 3 (its poll limit is 150 x 0.1 s, `:171-173`).
2. The master must be drained or pi blocks on a full pty buffer (needs the read loop).
3. `LANG`/UTF-8: tmux's `-u` (hs-d463) existed because tmux decided the charset from the client locale; a bare pty has no such layer, but pi's own width/charset choice from `LANG` is `unverified`.
4. Cleanup: the pty child must be a session leader so `killpg` stops pi and what it started (`fake cpd`, `sleep 60`); on macOS Apple platform binaries hide their environment from `ps -E` (docs/implementing.md:139), so the Gate scan may not see a straggler: the script must wait for its own pid group, as `wait_panes` does now (`pi-check.sh:92-104`).
5. It is a change to the named Gate exception's mechanism: the real pinned pi still runs; what is lost is "pi in a real multiplexer pane", which the adapter never used.

If the User prefers to keep the script as is, **tmux stays only in the Gate step** (`package.json:15`, before nextest, outside the tripwire) and the 8 tests of cluster A go to the PATH fake tmux (about the same win for the TypeScript tests, none for the Gate step).

### 2.3 Question (3): hook points for the tripwire, the static sleep check and the allowlist

`package.json:14` `verify` = `scripts/gate-run.sh npm run verify:steps`; `package.json:15` `verify:steps` = `check-migrations-frozen.sh && check-portable.sh && npm run typecheck && (t=$(mktemp -d) && TMPDIR=$t npm test; ...) && scripts/pi-check.sh node_modules/.bin/pi && export CPD_COMMIT=$(git rev-parse --short HEAD) && scripts/cargo.sh nextest run --workspace`.
`pi/test/gate.test.ts:12-24` pins that string verbatim, and `docs/implementing.md:171` pins the order: **any new step edits all three**.

**Tripwire `tmux` (nextest step only)**

- Where: the **last segment** of `verify:steps` (`package.json:15`), as `PATH="$PWD/scripts/tripwire:$PATH" scripts/cargo.sh nextest run --workspace` (or `env PATH=...`). Then `npm test`, `pi-check.sh` and the typecheck (earlier segments) never see it; `cargo` and `sccache` do (`scripts/cargo.sh:` starts sccache with `env -i` and only `PATH`, `HOME`, `TMPDIR`, `SCCACHE_*`, `docs/implementing.md:170`), which is harmless: neither calls tmux.
- Why PATH works for the harness: `TestEnv::command` builds `PATH = [env.bin()] ++ inherited_path()` (`cli/tests/common/mod.rs:854-856`), so a test's **fake tmux in `env.bin()` still wins** and only a call that reaches the real tmux hits the tripwire. `inherited_path()` (`:299-302`) drops only cpd git-wrapper dirs (`:283-291`), not the tripwire dir. `real_tmux()` (`:1273-1275`, 19 call sites by the `rg` below) takes the **first** `tmux` of that PATH, so it would return the tripwire: the tests that call it are the ones to name (`rg -n "real_tmux\(" cli/tests`).
- What it does: a `tmux` script (shell, no `timeout`/`setsid`, no `tmux` in command position so `check-portable.sh:104-151` is not triggered) that prints `tripwire: real tmux called by $NEXTEST_BINARY_ID $NEXTEST_TEST_NAME` to stderr, appends the same line to a log, and exits 127. Both variables are set by nextest 0.9.148 (`strings` of `cargo-nextest` shows `NEXTEST_BINARY_ID`, `NEXTEST_TEST_NAME`; the repo already relies on `NEXTEST_TEST_NAME`, `scripts/gate-run.sh:132,134`) and survive `TestEnv::command`, which removes only `CPD_`, `PI_`, `GIT_`, `XDG_`, `PLANNOTATOR_`, `GH_`, `GITHUB_`, `SSH_`, proxy variables, `TMUX`, `TMUX_PANE`, `ZDOTDIR` (`mod.rs:841-853`).
- A test that swallows the failing call would still pass, so also make the **run** fail: the log is a path under the run's `$TMPDIR`. `scripts/gate-run.sh` already turns any path left in `$RUN` into `FAIL ... path` (`:115-121`); to name it, print the log's lines in `finish()` after line 121 (before `rm -rf "$RUN"`, `:141`). Put the log at `$RUN/tripwire.log` (`$RUN` = `$TMPDIR` of the run, `gate-run.sh:26-30`; the tripwire reads `$TMPDIR`).
- Where the file lives: `scripts/tripwire/tmux` (a directory: `check-portable.sh:79-92` only iterates `[ -f ]` of `scripts/*`, so a subdirectory is not "not classified"; add `tripwire/tmux` to `covered`, `check-portable.sh:17`, to have it checked).

**Allowlist of legacy tests**

- `NEXTEST_BINARY_ID` + `NEXTEST_TEST_NAME` (e.g. `cpd::tui` + `bare::...`) identify the caller. The tripwire reads `scripts/tripwire/allow.txt` (one `<binary id> <test name>` per line, or a prefix such as `cpd::tui bare::`) and `exec`s the real tmux (found with `command -v` after removing its own dir from PATH) when the pair matches; it fails otherwise. Keeping a list in git makes each entry a visible debt that the T-class PRs remove. Entries come from the other inventories (`tests-mock-tui-a/b.md`, `tests-mock-session.md`, `tests-mock-flow-*.md`, `tests-mock-tooling.md`).
- It composes with `.config/nextest.toml:71-80` (the `tmux-restart` group lists `binary_id(cpd::tui) and test(/^bare::/)` and one install test): the same filter strings are the first allowlist entries.
- Do not parse `.config/nextest.toml` for the allowlist (a protected path, `docs/implementing.md` Tests).

**Static check for sleeps**

- TypeScript: the existing guard `pi/test/no-fixed-sleeps.test.ts:7-15` scans `pi/test/*.ts` (not `pi/test/compact-tools/`, not `pi/skills/`) for `settle(` and `sleep(`. Extend it in place (it runs in `npm test`, `package.json:10` file list, so **no `verify:steps` or `gate.test.ts` edit**) to also refuse `setTimeout(`, `setInterval(`, `Date.now()` deadlines, `spawnSync("sleep"` and `waitFor` polls outside `fake.ts`, with an allowlist of `file:line` debts (today: `compact-tools.test.ts:705-718`, `editor-box.test.ts:209`, `slack-channel.test.ts:99`, `follower.test.ts:34,72`, `handle.test.ts:13`, `pi-check.test.ts:315`).
- Rust: either a node test (same file) that `rg`s `cli/tests` and `core/src` test modules for `thread::sleep|std::thread::sleep|Instant::now` with an allowlist (no `verify:steps` edit), or a new script `scripts/check-test-sleeps.sh` inserted **before the last segment** of `verify:steps` (between `scripts/pi-check.sh` and `export CPD_COMMIT`, `package.json:15`), which needs `gate.test.ts:14-24`, `docs/implementing.md:171`, and an entry in `check-portable.sh:17` `covered`. The first is cheaper (Convention over configuration, DRY: one guard file already exists).
- Shell sleeps in shims (`sleep 60` in a fake body) are processes our shims start and are allowed if reaped (`nextest.toml:24`, `leak-timeout` fails a test that leaks), so the check targets the test's own thread, not shim text.

### 2.4 Question (4): what a new check must follow

From `scripts/check-portable.sh` and `docs/implementing.md`:

1. **Classification.** With no argument `check-portable.sh` fails (`:79-92`, `scripts/<name>: not classified`) any file directly in `scripts/` that is in neither `covered` (`:17`) nor `linux_only` (`:20-23`): a new `scripts/check-test-sleeps.sh` must be added to one of them, with a reason if Linux-only. `gate-run.sh` itself is `linux_only` (`:23`: it reads `/proc`; its macOS branch `ps -E`). A **covered** script may not use `/proc/`, `readlink -f`, `stat -c`, `date -d`, `sed -i`, `sed -r`, `flock`, `timeout`, `setsid`, `xargs -r`, `grep -P`, `sha256sum`, `nproc`, `mv -T` and the rest of the table (`check-portable.sh:30-55`); exemption only with `# portable: <reason>` at the line end. Needs only `sh`, `awk`, `mktemp`.
2. **tmux in command position** must be followed by `-u` and go through the script's one tmux function (`check-portable.sh:104-151`, docs/implementing.md:234 and "The row that guards tmux"). A tripwire that only prints and exits has no `tmux` in command position; one that forwards to the real tmux passes the caller's argv, so that line needs `# portable: forwards the caller's own argv`. A Rust test already refuses a tmux spawn outside `core/src/host/tmux.rs` (docs/implementing.md:234).
3. **Gate order is pinned** in three places: `package.json:15`, `pi/test/gate.test.ts:12-24` (exact string) and `docs/implementing.md:171` ("in that order, each stopping the run at its first failure"). A step added or a PATH prefix on the nextest segment edits all three. `docs/implementing.md:171` also says the Gate has no separate `cargo build`.
4. **gate-run.sh contract** (`docs/implementing.md:135-141`, `scripts/gate-run.sh:1-19`): everything the run starts carries `GATE_RUN` and a `TMPDIR` under the run dir; a clean run prints nothing; a leftover (process with `GATE_RUN`, path in `$RUN`, socket `*-<id>-*` in `/tmp/tmux-<uid>` or `$TMUX_TMPDIR/tmux-<uid>`) fails the run, is stopped and removed, names the test through `NEXTEST_TEST_NAME`. A new check that creates files must create them under `$TMPDIR` and remove them (anything left fails the run), and a child it starts must inherit `GATE_RUN` (do **not** use `env -i` without `GATE_RUN`: `pi-check.test.ts:37` does, which is why its leaks are invisible). The macOS scan cannot see Apple platform binaries (`/bin/sh`, `/bin/sleep`): do not rely on `GATE_RUN` to catch a leftover `sleep` shim there.
5. **Tests** (docs/implementing.md Tests, "A test must fail first, for the right reason"): a new guard ships its own tests in `cli/tests/tooling/` (like `gate_run.rs`, `pi_check.rs`: a script under test with fake externals on PATH, `TestEnv`) or in `pi/test/` (like `no-fixed-sleeps.test.ts`), listed in the area table (`tooling`: `*_script`, `gate_run`, `pi_check`, `harness`, `dead_code`); no `static`/`set_var`; `common::write_executable`; a test that must not overlap goes in a nextest test group with the reason beside it.
6. **Speed and leaks**: `leak-timeout` 100 ms fails a test that leaves a process holding its pipes (`nextest.toml:24`); `slow-timeout` kills at 90 s (`:33`); a new allowlist entry must keep `cargo nextest list -E '<filter>'` honest (`:41-56` says a renamed test drops out silently).

## 3. Tests that test an external (class X)

| test | what it really tests | disposition |
|---|---|---|
| `pi-check.test.ts:81` "pi-check passes against the current adapter and the pinned pi ... and fails when the adapter requires a field its source lacks" | the pinned pi's activation in a real tmux: the Gate's own step (`package.json:15`), run a second time inside `npm test` | delete the real-pi half (the Gate step covers it); keep the negative half as a unit test that `answers.ts` lacks what `parseSession` requires, or on the fake pi and fake tmux |
| `pi-check.test.ts:177-191` "a machine without tmux/node fails step 0" | only `command -v` and our message; no external runs | keep for node; delete for tmux if cluster 2.2 lands; else keep (no process starts) |
| `pi-check.test.ts:321-341` "every tmux call of pi-check is a UTF-8 client" | wraps the **real** tmux (`lookup("tmux")`, `:328`) to log argv | same test on the PATH fake tmux (argv only): not X, class T |
| `mainguard.test.ts:49-233` fixtures | git's `init`/`worktree`, not our code: our code reads `lstat` | class G, but replaced by `mkdir .git` (no git, no contract test) |
| `follower.test.ts:147-163` "a follower that cannot be started" | the OS's ENOENT from `spawn("cpd-does-not-exist")` | on an in-process fake `spawn` that emits `error` with `ENOENT` (our code handles `child.on("error")`, `follower.ts:141`); not X after cluster D |
| `skill.test.ts` (35) | docs and skill text against code (`git`, `gh`, `tmux` as words) | not X: it tests our docs; K |

Nothing in `pi/test/compact-tools` tests pi, node or the terminal itself: `@earendil-works/pi-coding-agent` and `pi-tui` are imported as libraries and exercised through our extension (unverified for the 51 files not read line by line: `rg` found no `child_process`/`net`/`http` import in them).

## 4. Leak and flake risks

1. **`pi-check.test.ts` leaks are not caught by the Gate** (above): env without `GATE_RUN` (`:37`), socket `pi-check-x-<pid>` (`pi-check.sh:55`), dirs in `/tmp` (`:14`). Each of its 8 tmux tests starts a private server and a real pi or fake pi in a pane; a test killed by the 80 s `spawnSync` timeout (`:35`) kills the script with SIGTERM, `trap 'exit 130'` (`pi-check.sh:134`) runs the cleanup, but a SIGKILL of the node runner (the 90 s nextest-style limit does not apply to `node --test`: `npm test` has no timeout of its own, `package.json:10`; `unverified` whether the Gate wraps it) would leave the server and the 3 s writer. Under the Gate's load (40-120) the script's 15 s activation limit (`:171`) and the test's `<30 s` bound (`:269`) are load-dependent: **flake**. The test `:292` removes its dir in a retry loop with `spawnSync("sleep")` (`:310-317`), itself a sleep.
2. **Fixture tests are load-dependent**: each `cpd` call is a node process (startup 50+ ms, under load 40-120 several hundred ms) against adapter limits of 300 ms (`requestTimeoutMs`, `fake.ts:250`; `callTimeoutMs: 300` in `start.test.ts:220`; reload `timeoutMs: 300`, `reload.test.ts:319`; `shutdownWaitMs: 200`). The reload test says so itself (`:322-325`, "how long the machine takes to load it is not the claim"). Tests with negative claims ("not restarted", "nothing sent") rely on `quiet()`/`drain()` markers, not sleeps (good, `no-fixed-sleeps.test.ts:5-6`), but each marker still waits real node round trips.
3. **A Fixture that is not closed leaves node followers**: `close()` is registered with `t.after` in most tests (`new` vs `.close()` counts equal in 14 files); files where `new Fixture(` outnumbers `.close()` (`reminder.test.ts` 1 vs 7, `askguard.test.ts` 2 vs 27, `noblock.test.ts`, `testguard.test.ts`, `mainguard.test.ts`) share one Fixture or use a helper (`started(role)`), `unverified` that every path closes. A failure between `new Fixture()` and `t.after(...)` leaks the follower (`follower.pids`, `fake.ts:26`), whose loop never exits on its own (`for (;;)`, `fake.ts:43`); the Gate leak check names it only if it carries `GATE_RUN` (it inherits the test's environment: yes).
4. **`Fixture` mutates `process.env`** (`PATH`, `FAKE_CPD_DIR`, `CPD_SESSION`, `CPD_HOME`, `PI_CODING_AGENT_DIR`, `fake.ts:120-127`) and restores it in `close()` (`fake.ts:299-303`): two Fixtures alive in one file (a second adapter: `load(options, pi)`, `fake.ts:244`) share it; node runs the tests of a file one at a time (default), so safe now, a trap if tests go concurrent.
5. **`setInterval` timers**: `reload.test.ts:212,251,391` mock only `setInterval`; `reload.ts:109` `setTimeout` and `startup.ts:226` stay real. A test failing mid-way can leave the poll interval keeping the node process alive until the file's timeout (`pollAdapter` stop is tied to `session_shutdown`, `reload.test.ts:199`).
6. **`HANGS`** (`reload.test.ts:314`) is killed by `process.kill(-pid, "SIGKILL")` after 300 ms (`reload.ts:110-114`): reaped by the production code, so no leak, but under load 40-120 the 300 ms may elapse before the child starts and the claim "times out" holds trivially.
7. **`render-golden.test.ts:72-75,418` and `indicator-harness.ts:24-47`** replace `Date.now` and `performance.now` globally for the file: safe per file process; `unverified` that every file restores them on a failing test.
8. **Gate run duration**: `npm test` runs inside `TMPDIR=$t` (`package.json:15`); `pi-check.test.ts`'s `/tmp` dirs and `/tmp/tmux-<uid>` sockets escape it.
9. `gate.test.ts:12-24` pins the Gate string: a change to wire the tripwire fails it first, by design (a test that fails first for the right reason).

## Plan of Jobs suggested by the counts (the design Job decides)

| Job | content | tests touched |
|---|---|---|
| 1 | cluster A+B: PATH fake `tmux` and fake `sleep` for `pi-check.test.ts`; fix its env (`GATE_RUN`, `$TMPDIR`) | 11 |
| 2 | cluster C: `mainguard.test.ts` without git | 17 |
| 3 | cluster D part 1: in-process fake `cpd`/follower and event-driven `waitFor` in `fake.ts`, `answers.ts` kept | `fake.ts` + about 20 (answers, follower) |
| 4-6 | cluster D part 2: migrate the 21 Fixture files (delivery, plan-review, review-chat, state, askguard, reload, updates, start, ...) | about 310 (mostly call-site edits) |
| 7 | clusters E+F+G: clock seams in `startup.ts`/`reload.ts`, compact-tools real timeouts, `slack-channel` | about 15 |
| 8 | the tripwire and allowlist (`scripts/tripwire/`, `package.json:15`, `gate.test.ts`, `gate-run.sh` finish, docs) | tooling tests |
| 9 | the static sleep check (extend `no-fixed-sleeps.test.ts`) | 1 |
| opt. | `pi-check.sh` on a python3 pty (2.2) | script + 4 Rust + 3 TS tests |
