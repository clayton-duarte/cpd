# W5 (observability, metrics, notifications, safety, user interaction): research report

Read at: 2026-10-12. Clones in `/tmp/cpd-research/repos/` (SHAs below). Items reviewed: 20 of 20 listed (herdr-resurrect and the two agentbox repos read at lighter depth). Items not reviewed: none. Extra items reviewed: 16 (see "Additional finds"). Not run: nothing was installed, executed or linked. All findings come from source, manifests, and GitHub/npm API data.

Stars/downloads below are from 2026-10-12 (`gh api`, npm `last-month`). "Contributors" is `gh api .../contributors` length (bus-factor proxy).

## Summary

### Ground truth that shapes every metrics/notification judgement (verified, not from READMEs)

- **Pi's own session JSONL already contains everything N13 needs.** I read a live session file (`~/.pi/agent/sessions/--Users-claytonduarte-projects-cpd--/2026-10-12T20-53-01-222Z_01a131e3-….jsonl`, read-only). Header line: `{"type":"session","version":3,"id","timestamp","cwd"}`. `model_change` carries `provider`/`modelId`. `thinking_level_change` carries `thinkingLevel`. Each assistant `message` carries `usage:{input,output,cacheRead,cacheWrite,totalTokens,cost:{input,output,cacheRead,cacheWrite,total},reasoning}`, `stopReason`, `durationMs`, `thinkingLevel`, `provider`, `model`, `timestamp`. So per Attempt: turns = count of assistant messages, tokens/cost = sum of `usage`, duration = last − first timestamp (or sum of `durationMs`), Tier = model + thinkingLevel. No provider API and no herdr event is needed.
- **Pi also exposes it over RPC:** `get_session_stats` returns `tokens{input,output,cacheRead,cacheWrite,total}`, `cost`, `toolCalls`, `assistantMessages`, `contextUsage` (pi docs `rpc-commands.md:526-563`). Only useful if the Worker is run in RPC mode, which herdr panes are not.
- **herdr events carry status, not usage.** Subscribable pane events are `pane.agent_status_changed` etc. (`socket-api.mdx:822-836`). Event history is not durable and is delivered in bounded batches (`socket-api.mdx:794-795`). So herdr is the *trigger*; the pi session file (found via `pane.get` → `agent_session.path`, see herdr-token-dashboard `main.go:109`, agent-usage `omp/provider.go:4-6`) is the *data*.
- **Pi has a final, notification-only `agent_settled` event** (pi `extensions.md:64-67`), meaning "Pi will not continue automatically". Three independent projects hook it for completion (herdr-pings, shepherd-pi, and herdr-pings's scoreboard). herdr's own `done` is a terminal-state rollup; `agent_settled` is more exact for "Attempt turn finished".
- **Read-only Reviewer (N4) is upstream-native:** `pi --fork <id> --tools read,grep,find,ls` (pi `docs/cli.md:114,128`; `pi --help` lines 29, 37). No extension needed for the read-only tool set; an extension is only needed if the Reviewer must also run read-only bash (then `@pedro_klein/pi-readonly-bash`, 30 dl/mo, is the only candidate seen).

### Best candidate per need

| Need | Best item | Coverage | Notes |
|---|---|---|---|
| N2 completion notice that doesn't clobber input | `ryonakae/shepherd` (`@ryonakae/shepherd-pi`) | partial | Pi extension wakes the owner Lead only when `ctx.isIdle()` and after `agent_settled`; delivers via `pi.sendMessage(..., {deliverAs:"followUp", triggerTurn:true})` (`packages/shepherd-pi/src/index.ts:229-339`). Never touches editor text (no editor API calls in code; whether a half-typed buffer survives a triggered turn is **unverified**). Weak maturity (37+55 dl/mo, 1 contributor, last push 2026-08-28). Pattern is worth copying; the package is not. |
| N2 (pull variant) | `joelhooks/herdr-pings` | partial | Pi extension appends `turn_ended`/`turn_error` JSONL to a per-pane spool on `agent_settled`; `herdr-ping-wait` CLI consumes with byte-offset cursors (`herdr-turn-ping/index.ts:149-181`). Fits a deterministic scheduler better than an LLM wake. |
| N3 Worker→parent question, blocking | `trim21/pi-extensions` `talk` module | partial | `talk-ask` blocks until reply (30 min default), SQLite mailbox, presence via pid+start time, delivery `steer`/`nextTurn` (`src/talk/index.ts:204-208`). Personal repo, "breaking changes at will". Pattern, not dependency. |
| N3 (permission asks only) | `gotgenes/pi-packages` `pi-permission-system` | partial | Forwards `ask` prompts from child pi processes to the parent session through a file inbox + heartbeat, keyed by env `PI_SUBAGENT_PARENT_SESSION` (`docs/subagent-integration.md:47-50,166`). Solves permission asks only; it is the best-documented parent/child convention seen. |
| N13 per-Attempt metrics | none ready-made; **write ~50 lines** over the session JSONL. Closest: `naturalmoods/herdr-burnlog` | partial | BurnLog: SQLite, idempotent, reads Pi session format 3, records tokens + recorded cost with provenance (`burnlog.py:176-181`), triggered by `pane.agent_status_changed`. But keyed by project/model, not Job/Attempt/Tier. Parsing code to borrow: `senna-lang/herdr-agent-usage` `internal/providers/omp/extract.go:13-23`, `Davidcreador/herdr-token-dashboard` `main.go:1218-1330`. |
| N13 live trace view | `langfuse/pi-observability-plugin` | partial | Per-turn traces with tokens/cost/TTFT; subagent nesting via inherited env (`src/index.ts:752-762`). Needs a Langfuse backend and credentials; overkill for tuning defaults. |
| N14 structured questions to User | `juicesharp/rpiv-mono` `rpiv-ask-user-question` | full (questions) / none (plan approval) | `ask_user_question` tool, 1–4 questions × 2–4 options, previews, notes, TUI/RPC/ACP (`ask-user-question.ts:377-412`). 299k dl/mo. No dedicated plan-approval UI; markdown `preview` can approximate it. |
| N14 (rich/async form) | `nicobailon/pi-interview-tool` (`pi-interview`) | partial | Browser/Glimpse form; `async: true` returns immediately and delivers the answer later via `pi.sendMessage` (`index.ts:352,1179`). 21k dl/mo. |
| N15 visibility | `hhdebb/herdr-radar` + `jeffarese/herdr-bar` | partial | Radar rewrites the sidebar Agents panel (`agent.view.set`, tokens). Bar adds Cmd+K jump, per-tab status glyph. Neither knows Jobs/DAG. Pi icon supported in both. |
| N15 custom status text from pi | `SpatialNexus/herdr-subagent-state` (pattern) | pattern | Pi extension calling `herdr pane report-agent --custom-status` / `report-metadata` (`hooks/herdr-subagent-state.ts:98`). Shows how the Lead extension can publish "J3·A2 reviewing" to the sidebar. |
| N16 sandbox (macOS) | `erichll/pi-packages` `@erichll/pi-sandbox` | partial | Seatbelt (macOS) / bwrap (Linux) via `@anthropic-ai/sandbox-runtime`; overrides pi's bash operations and hooks `tool_call` (`src/index.ts:254-262,516,531`). Peer `pi ^1.1.0`. Its subagent provider wraps *its own* subagents or `pi-subagents`, not arbitrary herdr panes. |
| N16 policy | `gotgenes/pi-packages` `pi-permission-system` | partial | `tool_call` allow/ask/deny with bash AST parsing, per-agent frontmatter, fail-closed. `path_write: {"*":"deny"}` is a documented read-only posture. No OS isolation. |
| N16 destructive-command guard | `kenryu42/cc-safety-net` | partial | `pi.on('tool_call')` blocklist (`src/hosts/pi/tool-call.ts:54`). Fail-open by design. |
| N17 agent-agnostic | `ivolkoff/herdr-huddle` (questions), `dcolinmorgan/herdr-remote` (monitoring) | partial | Both talk to panes/CLI, not to pi internals. |

### Overlaps and conflicts

- **Same hook, different owners.** `pi-permission-system`, `cc-safety-net`, `@erichll/pi-sandbox` and `trim21` bwrap all hook `tool_call` / bash operations. Layering is possible (gotgenes explicitly composes "most restrictive wins" across its own layers) but ordering across *different* packages is unspecified; **unverified** whether two blocking `tool_call` handlers compose cleanly.
- **Same sidebar slot.** herdr-radar (`agent.view.set`, source `plugin:…`), herdr-bar (renames tabs), herdr-auto-title (renames tabs and panes) and herdr-agent-usage (`report-metadata` tokens `$context/$limit/$cache`) all write presentation state. herdr-bar documents a "renaming guard" against competing naming plugins (`src/herdr_bar/renaming_guard.py`); radar's header comment warns that `agent.view.set` disables the panel's own toggle. Pick one owner for tab names and one for the Agents panel order.
- **Same state channel.** `herdr-subagent-state` reports `herdr pane report-agent` for pi under its own `--source`; herdr's official pi integration also reports state. The plugin's README says to set `HERDR_SUBAGENT_STATE_METADATA_ONLY=1` for pi to avoid fighting.
- **Notification duplicates.** `herdr-agent-usage` (`run-notify.sh`), herdr-focus-notify, herdr-ntfysh, termaxa and herdr-remote all subscribe to `pane.agent_status_changed` and notify the human. None of these notify the Lead agent; they target the User.
- **Tool name clash risk (pi):** `ask_user_question` (rpiv) vs `interview` (pi-interview) vs `permissions:*` prompts: different names, but all open blocking UI; **unverified** how two simultaneous blocking prompts queue (pi-interview has its own multi-agent queue; gotgenes queues forwarded prompts behind open dialogs, `README.md` "Forwards prompts from subagents").
- **License trap:** `dcolinmorgan/herdr-remote` is AGPL-3.0-or-later (dual-licensed commercial). Do not copy code into CPD.

### Recommendation for CPD v0

1. **N13: build, don't adopt.** Write a small collector that, on Attempt end, finds the Worker's session file via `pane.get` `agent_session.path` and sums `usage.*`, counts assistant messages, takes first/last timestamps, records `model` + `thinkingLevel` (Tier). Store next to the Job-graph history. Borrow the parse shape from `herdr-agent-usage/internal/providers/omp/extract.go:13-23`. Skip Langfuse/MLflow/OTel for v0 (needs a backend).
2. **N4: use pi built-ins** (`--fork … --tools read,grep,find,ls`). Add `pi-readonly-bash` only if Reviewers need to run commands; the Gate already runs tests.
3. **N2: copy the `agent_settled` + spool pattern** from `herdr-pings` (append `turn_ended` with tokens to a per-pane JSONL; scheduler waits on it), and the `isIdle()` + `followUp` deferral from `shepherd-pi` for waking the Lead. Test explicitly whether a half-typed Lead editor buffer survives a `triggerTurn` (unverified in code).
4. **N3: reuse the `pi-herdr-subagents` escalation (`ask_question`/`caller_ping`) from the main spec**; borrow gotgenes' `PI_SUBAGENT_PARENT_SESSION` env convention and heartbeat/liveness design, and trim21's `talk-ask` bidirectional-ask arbitration if a Lead↔Worker deadlock is a concern.
5. **N14: install `@juicesharp/rpiv-ask-user-question` on the Lead** for Context-phase interviews and plan-approval choices (approve / edit / reject with a markdown preview of the Job list). It is the most-used and best-tested option here. Plan approval is not a dedicated feature; confirm in a spike.
6. **N15: nice-to-have only.** Adopt `herdr-radar` or `herdr-bar` for sidebar legibility; use the `report-agent --custom-status` / `report-metadata` pattern in the Lead extension to show Job/Attempt/Tier text. Do not build a DAG view (set aside by the main spec).
7. **N16 (post-MVP):** best macOS route is `@erichll/pi-sandbox` or running the Worker's `pi` under Anthropic's `srt` (`@anthropic-ai/sandbox-runtime` ships a `srt` CLI, `npm view … bin`); the `srt`-wraps-whole-pi-process idea is **guess:** unverified. Pair with `pi-permission-system` frontmatter policy per role. Container/microVM options (agentbox, `sbx`) are heavier and, except agentbox, do not list pi.
8. **Skip** for CPD: zoetrope (Claude/Codex only), termaxa (no pi hook), herdr-agent-progress (Claude/Codex only), herdr-auto-title / herdr-bar tab renaming (cosmetic), herdr-resurrect (crash restore; optional later), herdr-remote (AGPL, human-facing).
9. **Watch-outs:** `pi-permission-system` is at v40 five months after creation (high churn) with 117 open issues/PRs; pin versions. `@erichll/pi-sandbox` peers on pi `^1.1.0` (matches the installed 1.1.0) but is 11★ and 10 weeks old.

## Items

### furkankly/zoetrope (crates.io `zoetrope`; Homebrew `furkankly/tap/zoetrope`)
- Meta: ★1,032 · pushed 2026-10-06 · created 2026-08-18 · MIT · v0.2.0 (2026-09-10) · 2 contributors · downloads n/a (Rust crate, not npm) · SHA b1f31dd
- What: Rust TUI/WASM that replays or follows a Claude Code or Codex transcript as a live flow graph of agents and tool calls.
- Mechanism: providers are `src/provider/claude` and `src/provider/codex` only (directory listing); no pi provider. Herdr plugin (`herdr-plugin/herdr-plugin.toml`) is a pane/action bridge: overlay pane `graph`, actions `open|open-split|open-tab|setup-keys`; resolves the focused pane's `agent_session` with `pane.get` (`herdr/resolve.sh:12-33`, `min_herdr_version = 0.8.2`). Reads `output_tokens`/cache fields from Claude usage (`src/provider/claude/wire.rs:225-231`).
- Maturity: CI (ci/cd/release-plz), CHANGELOG, `benches/`, release cadence active; tests dir absent (inline tests unverified).
- CPD fit: N15 only as an idea (session as a graph). Zero for pi sessions.
- Gaps/risks: no pi parser; a pi provider would be a new Rust module; read-only, no data export.
- Verdict: SKIP: Claude/Codex only; useful as design inspiration for a session graph.

### jeffarese/herdr-bar
- Meta: ★59 · pushed 2026-10-10 · created 2026-07-24 · MIT · v0.7.3 (2026-10-10) · 3 contributors · n/a · SHA e224550
- What: Python plugin: Cmd+K fuzzy jump to any agent/tab/pane with live status, auto tab titles, per-tab status glyph in the tab bar.
- Mechanism: pane overlay `bar` (popup) via `plugin pane open` (`herdr-plugin.toml`); `[[startup]]` + `pane.agent_detected` event start a watcher (`--start-titles`); socket client `src/herdr_bar/client.py:78-160` (socket with CLI fallback); subscribes `pane.agent_detected` + per-pane `pane.agent_status_changed` (`tab_status.py:272-280`); renames tabs from the highest-urgency agent status (`tab_status.py:52-63,167-190`). Pi is one of the icon set (README). `min_herdr_version 0.9.0`.
- Maturity: CI, 15+ test modules (`tests/`), 0 open issues, 7 releases in ~11 weeks (guess from tag v0.7.3).
- CPD fit: N15 partial (status at a glance, elapsed time per row, blocked/finished agents float to top). No Job/Attempt concept.
- Gaps/risks: rewrites tab names (conflicts with other naming plugins; has a guard); Python 3 only.
- Verdict: BORROW: adopt as-is for human navigation if desired; copy its snapshot-diff-on-event pattern for status UIs.

### termaxa/termaxa
- Meta: ★24 · pushed 2026-10-09 · created 2026-07-04 · Apache-2.0 OR MIT (README) · v0.21.3 · 2 contributors · crates/brew (no npm) · SHA a47e038
- What: Rust command gate for agents: policy allow/ask/deny, preview of blast radius, backup + rollback, hash-chained audit log.
- Mechanism: hook binary invoked by the agent's own hook path (README "Harnesses" table: Claude Code, Codex, Cursor, Copilot CLI; generic `termaxa run -- <cmd>` / `termaxa wrap -- <agent>` shell shimming). **No pi row** in the table. Herdr plugin (`herdr-plugin/herdr-plugin.toml`): `[[startup]] watch.sh` follows the record, `[[events]] pane.agent_status_changed` → `on-status.sh`, pane `record` overlay; actions start the agent under the gate. min herdr 0.9.0.
- Maturity: CI, `tests/`, SECURITY.md, `docs/dialects.md`, rapid releases (v0.21 in 3 months), 13 open issues.
- CPD fit: N16 only. Enforcement is a *command hook with policy*; for pi it would run as `termaxa wrap -- pi` which shims shells "resolved by name" (README) → a pi bash tool that uses an absolute shell path escapes it (**guess:** unverified for pi).
- Gaps/risks: hook fails open on unreadable payload (README); no pi dialect; backups copy files (disk cost per Worker).
- Verdict: SKIP: no pi integration, and `tool_call` hooks (cc-safety-net) are the native route in pi.

### senna-lang/herdr-agent-usage
- Meta: ★48 · pushed 2026-10-11 · created 2026-07-16 · MIT · v0.5.20 (2026-10-11) · 12 contributors · n/a · SHA fc01eab
- What: Go plugin: sidebar context meters, prompt-cache row, provider rate-limit windows and low-allowance toasts for Claude, Codex, OMP, **Pi**, Grok, Hermes, etc.
- Mechanism: events `pane.agent_status_changed` (→ `run-status.sh`, `run-notify.sh`) and `pane.focused`; `[[startup]]`; pane `limits`; `[[build]]` `ensure-binary.sh` (`herdr-plugin.toml`). Finds the pi session via herdr `agent_session.kind="path"` (`internal/providers/omp/provider.go:4-6`), parses assistant `usage` rows (`omp/extract.go:13-23`: `input, output, cacheRead, cacheWrite, totalTokens, cost.total`, `provider`, `model`, `stopReason`, `timestamp`; `types.go:21-26` `UsageEvent` per turn). Publishes via `herdr pane report-metadata --source … --token name=value` (`internal/herdrcli/herdr.go:392-402`) and `herdr notification show` (`herdr.go:405-407`). Requires `herdr integration install pi` for session matching (README).
- Maturity: strongest of the group: ~123 `_test.go` files, CI with lint+govulncheck, contract-drift and model-drift workflows, commitlint, 12 contributors, release per day recently.
- CPD fit: N13 partial: the code proves the pi-session data path end to end, but it displays current context/cache/limits, does not store per-Attempt history. N15 partial (sidebar tokens `$context`, `$limit`, `$cache`; requires adding rows to herdr `config.toml`). Metrics extractable per session: tokens (in/out/cache r/w/total), cost (USD, recorded), model/provider, context-window %; **not** turns or duration.
- Gaps/risks: Go build step at install (toolchain or prebuilt download); `herdr-plugin.toml` touches many events (every focus change runs a script).
- Verdict: BORROW: best reference for pi session parsing and sidebar tokens; adopt the plugin itself only if the User wants live meters.

### Davidcreador/herdr-token-dashboard
- Meta: ★25 · pushed 2026-10-09 · created 2026-06-25 · MIT · v0.2.0 (2026-09-30) · 5 contributors · n/a · SHA 0387215
- What: Go Bubble Tea TUI plus `done` toast showing per-pane tokens/cost across Pi, OpenCode, Claude Code, Codex.
- Mechanism: pane `dashboard` (placement tab), event `pane.agent_status_changed` → `token-dashboard --notify` (`herdr-plugin.toml`); `herdr pane list` poll (`main.go:1065`), `agent_session` struct (`main.go:109`). `readPiSession` (`main.go:1218-1330`) parses `type:"message"` assistant rows (`usage.input/output/cacheRead/cacheWrite/cost.total`), `model_change`, `compaction`, tool-call blocks, first/last timestamp → duration, message count. Toast on `prev != done && curr == done && cost > 0` (`main.go:343-345`).
- Maturity: CI (gofmt, test, build), tests for claude/codex/session/view; no pi-specific test file seen by name (unverified); history persistence only for Claude (`history.go`). Closed panes vanish from the table (live-only).
- CPD fit: N13 partial (confirms the per-session extraction set: cost, in/out/cache tokens, model, messages, tool counts, compactions, duration), but live-only, no per-Attempt store.
- Gaps/risks: reads whole file each refresh (`os.ReadFile`); Claude/Codex costs are estimates from a built-in price table.
- Verdict: BORROW: copy the pi parser logic (30 lines); not a collector.

### yankewei/herdr-focus-notify
- Meta: ★29 · pushed 2026-10-11 · created 2026-06-27 · license not declared (GitHub `null`) · v0.8.0 (2026-10-08) · 6 contributors · n/a · SHA 94e8e13
- What: macOS clickable desktop notification when an agent is `blocked`/`done`; click focuses the pane.
- Mechanism: Rust binary; events `pane.agent_status_changed` + `pane.focused` (`herdr-plugin.toml`); reads `HERDR_PLUGIN_EVENT_JSON` (`src/main.rs:103`); `herdr pane list` (`src/focus.rs:466,491`); renders via external `alerter` (`src/notifier.rs`). Suppresses the toast when Herdr is frontmost and the pane is focused.
- Maturity: CI; `tests/cli_test.rs` 992 lines; CHANGELOG; min herdr 0.7.5; **no license file** (risk if copying).
- CPD fit: notifies the *User*, not the Lead. N2 only in the human sense ("don't spam if I'm looking at it"). The look-at-focus suppression logic is a useful idea for the Lead notification gate.
- Gaps/risks: macOS-only, needs `alerter`; unlicensed.
- Verdict: SKIP: human desktop alerts; the User only watches the Lead, who is covered by N2 mechanisms above.

### cobanov/herdr-ntfysh
- Meta: ★19 · pushed 2026-07-02 · created 2026-07-02 · MIT · v0.2.0 · **archived** · 1 contributor · n/a · SHA f074624
- What: Go binary that pushes an ntfy notification when an agent turns `done`/`blocked`.
- Mechanism: event `pane.agent_status_changed` → `./herdr-ntfysh` (`herdr-plugin.toml`); decision `internal/decide/decide.go:29-53` (done on herdr `done` or `working→idle`, blocked, per-pane debounce `internal/dedup`).
- Maturity: unit tests per package; archived upstream.
- CPD fit: none for the Lead; useful for the User away from desk (phone). Trigger = herdr state event, delivery = HTTP to ntfy.
- Gaps/risks: archived; other ntfy plugins exist (`horn553/herdr-ntfy`, `zom-2018/herdr-ntfy-notify`).
- Verdict: SKIP: archived and human-only.

### ntindle/herdr-resurrect
- Meta: ★38 · pushed 2026-10-05 · created 2026-07-06 · MIT · no releases · 3 contributors · n/a (also `package.json`) · SHA 109a937
- What: tmux-resurrect for herdr: periodic snapshots of workspaces/tabs/panes/cwd/programs/agents; restore after crash/reboot.
- Mechanism: actions save/restore/dry-run; `[[panes]] autosave`; events `workspace.created|closed`, `pane.created|closed|exited|agent_detected` → debounced `bin/on-event.js` (≤1 write/20 s); opt-in auto-restore keyed on the socket boot token (`herdr-plugin.toml`, its own comment notes herdr has no `server.ready` event). Restore relaunches allowlisted programs and agents; herdr already restores shape and `resume_agents_on_restore` natively (README table).
- Maturity: no CI file listed, no tests listed (`git ls-files`), 4 open issues.
- CPD fit: not in N1–N19; peripheral: crash resilience of a long Workflow (a Lead surviving a reboot). CPD's own history lives in the Job graph, so resurrection is optional.
- Gaps/risks: relaunch allowlist is a security surface.
- Verdict: SKIP: out of scope; herdr built-ins cover the basic case.

### eliasstravik/herdr-agent-progress
- Meta: ★31 · pushed 2026-09-15 · created 2026-09-15 · MIT · no releases · 1 contributor · n/a · SHA 7f3a3fe
- What: Rust publisher that shows an agent's self-reported % progress and activity in the herdr sidebar.
- Mechanism: hooks into Claude Code and Codex (SessionStart/PostToolUse/UserPromptSubmit `additionalContext`) to make the agent report progress; publishes with `pane report-metadata` (`src/publisher.rs:167`). `compatibility.md:22`: Pi is listed as "native extension events are the appropriate candidate… progress path not implemented". Marks reports stale after 5 min.
- Maturity: tests (`tests/cli.rs`), docs; macOS only; README says private repo access needed at install ("You'll need access to the private repo", README step 1) despite the public listing: unverified.
- CPD fit: N15 idea (agent-reported progress → sidebar). Not pi.
- Verdict: SKIP: no pi path; self-reported % is unreliable for scheduling.

### SpatialNexus/herdr-subagent-state
- Meta: ★0 · pushed 2026-07-06 · created 2026-07-06 · license none · no releases · 1 contributor · n/a · SHA a61bbb7
- What: installs native hooks for Claude, Codex and **Pi** that report `working` while sub-agents/tools run, so herdr doesn't flip to idle/done mid-delegation.
- Mechanism: copies `hooks/herdr-subagent-state.ts` into `~/.pi/agent/extensions/`. Pi hooks: `session_start`, `before_agent_start`, `tool_call`, `agent_end`, `session_shutdown` (`hooks/herdr-subagent-state.ts:146-150`). Calls `herdr pane report-metadata --source …metadata --agent pi --state-label working=active` and `herdr pane report-agent --source … --state working|blocked|idle --custom-status … --seq N` (`.ts:70-99`), using a monotonic `seq` from wall-clock ns and a per-pane state file. Counts `before_agent_start` as "sub-agent running" (**inference:** so it assumes in-process subagent tools).
- Maturity: 0★, one commit, one Python test file; Pi row says "enhancement-only" since pi has an official herdr integration.
- CPD fit: N15 pattern: shows exactly how a pi extension drives herdr's agent state + custom status text. N2 related: herdr may report `done`/`idle` while a Worker still waits on a child; relevant if Workers ever spawn helpers.
- Gaps/risks: zero traction; the `blocked` mapping on `agent_end` could make every finished Worker look "blocked" (it reports `blocked`+"awaiting input" when no work is active, `.ts:127-130`); would fight the official integration unless metadata-only.
- Verdict: BORROW: read for the `report-agent --custom-status --seq` pattern; don't install.

### hhdebb/herdr-radar
- Meta: ★168 · pushed 2026-10-09 · created 2026-09-07 · MIT · v1.4.4 (2026-10-09) · 14 contributors · n/a · SHA dd42772
- What: Node plugin that reshapes the sidebar Agents list: groups by project, nests worktrees, vendor icons, state marks that persist until seen, activity ordering.
- Mechanism: daemon with one long-lived `events.subscribe` socket connection used only as a wake hint (`lib/subscribe.js:5-40`; notes the 512-event replay on connect, the 10 events/s per-subscription throttle, and that subscribing `pane.updated` slows herdr writes ~110 ms vs 1–2 ms); writes display tokens via `report-metadata`; reorders the panel with raw socket `agent.view.set`/`clear` (`lib/view.js:3,50`, documented in `socket-api.mdx:370-440`); `configure` action writes managed blocks into herdr `config.toml`. Pi has a mark (README line 200). `min_herdr_version 0.9.0`.
- Maturity: CI, 18 test files, changelog, v1.4.4, three READMEs.
- CPD fit: N15 partial (best-looking visibility for many agents; worktrees nested under repo matches CPD's worktree-per-Attempt layout). Zero Job-awareness.
- Gaps/risks: edits `config.toml` blocks; takes over the Agents panel order; Node ≥18.
- Verdict: BORROW: adopt for legibility if wanted; copy the `events.subscribe`-as-wake-hint pattern (and its throttling notes) for any CPD daemon.

### kryptamine/herdr-auto-title
- Meta: ★250 · pushed 2026-10-12 · created 2026-08-24 · MIT · v0.13.2 (2026-10-12) · 13 contributors · n/a · SHA 8b25045
- What: Go daemon naming tabs/panes after work in them; also reports a workspace `$topic` sidebar token.
- Mechanism: `[[startup]]` daemon (`herdr-plugin.toml`), reads the session twice a second, socket client in `internal/herdr`, Claude transcript reader (`internal/claude/transcript.go`); no pi reader found (grep for `.pi/` in `internal/`: none). Tab rename with "rename-yourself-and-it-stops" rule; `$topic` via `workspace report-metadata` (README).
- Maturity: release-please, renovate, tests per package, CI.
- CPD fit: N15 cosmetic. Pane names like "Job J3 · Attempt 2" could come from CPD's own spawn instead.
- Verdict: SKIP: cosmetic, no pi reader; CPD should name panes at spawn.

### langfuse/pi-observability-plugin (npm: @langfuse/pi-observability-plugin)
- Meta: ★15 · pushed 2026-09-25 · created 2026-08-07 · MIT · v0.1.2 (2026-09-03) · 323,520 dl/mo · 2 contributors · SHA 5f3c083
- What: Pi extension exporting prompts, turns, generations (tokens, cost, TTFT, cache/reasoning splits), thinking and tool calls to Langfuse via OpenTelemetry.
- Mechanism: manifest `pi.extensions: ["./src/index.ts"]` (`package.json`). Hooks: `session_start` (`index.ts:826`), `before_agent_start` (837, creates the turn root span with `session_id`), `context`, `agent_start`, `before_provider_request` (913), `message_update`, `message_end` (967), `tool_execution_start/end` (1025/1051). Subagent lineage: publishes the turn root as `LANGFUSE_PI_PARENT_TRACE_ID/SPAN_ID/SESSION_ID/DEPTH` into `process.env` (`index.ts:752-762`); a child pi *process that inherits that env* nests under it (`index.ts:77-90`) and can also be fed `LANGFUSE_PI_TRACEPARENT` explicitly (`index.ts:42,97`). Whether herdr-spawned panes inherit the Lead's env is **unverified** (panes are started by the herdr server, so likely no; an explicit env at spawn would be needed).
- Maturity: tests (unit, integration, system-prompt…), CI incl. pi-version matrix, zizmor, release workflow; README says "experimental, breaking changes possible". 323k downloads suggests heavy use (note: Langfuse-vendor package).
- CPD fit: N13 partial and only with a Langfuse server (cloud or self-hosted) and keys in `~/.pi/agent/langfuse.json`. Per-Attempt metrics derivable from a trace per Worker session, with cost, tokens, duration, turn count; Job/Attempt/Tier would have to be injected as metadata (not supported by config; would need a wrapper env, unverified).
- Gaps/risks: external service; prompts/outputs leave the machine unless self-hosted; images uploaded as media (can disable via `LANGFUSE_MEDIA_UPLOAD_ENABLED`, `index.ts:38`).
- Verdict: BORROW: not for v0 (needs a backend); good fit later for retrospectives. The env-propagated parent-trace pattern is worth copying for Lead→Worker lineage.

### gotgenes/pi-packages (npm: @gotgenes/pi-permission-system)
- Meta: ★255 · pushed 2026-10-12 · created 2026-05-15 · license `null` in GitHub API (package says MIT in badge/fork; unverified in file) · pi-permission-system-v40.2.0 (2026-10-12) · 67,193 dl/mo · 22 contributors · 117 open issues/PRs · SHA c8f89b5
- What: Permission enforcement extension for pi: allow/ask/deny over tools, bash (AST-parsed), MCP, skills, paths, external dirs; fork of MasuRii's package.
- Mechanism: `pi.on("tool_call", …)` (`src/index.ts:391`), `before_agent_start` (388: hides disallowed tools through `pi.setActiveTools`, `index.ts:305`), `input` (389), `session_start/shutdown`; `pi.registerCommand("permission-system")` (`src/config/config-modal.ts:253`). Config: global `~/.pi/agent/extensions/pi-permission-system/config.json`, project `<cwd>/.pi/extensions/pi-permission-system/config.json` (project config only loaded once the project is trusted), per-agent YAML frontmatter `permission:` overrides both. Layers: `path` → `external_directory` → per-tool → `bash`, most restrictive wins; directional `path_write`/`path_read` (README: `path_write: {"*":"deny"}` = "read-only-agent posture"). Fails closed on gate error/unparseable bash. **Escalation:** `PI_SUBAGENT_PARENT_SESSION=<id>` env at spawn marks a child; child writes forwarded requests to a file inbox, parent polls and answers; heartbeat record under `<agent dir>/sessions/permission-forwarding/serving/` with 2 s grace before fail-fast (`docs/subagent-integration.md:47-50,166-190`; code `src/authority/approval-escalator.ts`, `forwarding-io.ts`, `forwarded-request-server.ts`). `promptTimeoutMs` auto-denies unanswered prompts for unattended runs; `promptNotifications: ["bell"]` rings terminal. Emits `permissions:ui_prompt`/`permissions:decision` events for other extensions.
- Maturity: 181 test files in package, biome+eslint+tsc, release-please-like changelog; very high churn (v40 in five months); peer `pi >=1.0.0`.
- CPD fit: N16 partial (policy-level; **no OS isolation**); N4 partial (read-only posture via `path_write deny` + bash allowlist per agent frontmatter, but pi `--tools` is simpler); N3 partial (permission asks only, but a well-specified parent/child convention that CPD can reuse for its own Escalation transport).
- Gaps/risks: 117 open issues; the `permission:` frontmatter only works if the spawner applies agent frontmatter or per-worktree project config (needs project trust); any spawner outside `pi-subagents` must set the env var itself; MCP/skills/special surfaces add config weight.
- Verdict: BORROW: use selectively (per-role policy later); copy the parent-session env + heartbeat convention for CPD Escalation. Not v0-critical.

### kenryu42/cc-safety-net (npm: cc-safety-net)
- Meta: ★1,604 · pushed 2026-10-12 · created 2025-12-25 · MIT · v2.6.10 (2026-10-12) · 52,231 dl/mo · 8 contributors · SHA c0596cd
- What: Blocks destructive shell commands (`git reset --hard`, `rm -rf` etc.) and secret-file access across ~16 coding CLIs, including Pi.
- Mechanism: `package.json` `pi.extensions: ["./dist/pi/index.js"]`; `src/hosts/pi/tool-call.ts:54` `pi.on('tool_call', handler)`; handler maps `bash`/`powershell` to a command parse and other tools to path checks, returns `{block:true, reason}` (`tool-call.ts:11-51,90-120`). Presets Standard/Strict/Paranoid via `npx cc-safety-net gui`. Fail-open on malformed payload (README mentions "A broken …"; `docs/residual-risk.md`).
- Maturity: 222 test files, codecov, evals dir, SECURITY.md, residual-risk registry, daily releases.
- CPD fit: N16 partial: cheap guardrail against Worker accidents (the exact `reset --hard` failure mode); not isolation. Enforcement = `tool_call` hook + command parser, deny-by-pattern.
- Gaps/risks: blocklist semantics (misses unknown dangerous commands); needs `dist/` committed (it is) or build.
- Verdict: REUSE (post-MVP N16 first step): drop-in for Workers; the lowest-effort safety win. Low confidence on stacking with other `tool_call` blockers.

### trim21/pi-extensions (npm: @trim21/personal-pi-extensions)
- Meta: ★3 · pushed 2026-10-12 · created 2026-06-20 · MIT · no GitHub releases (npm 0.1.2026…) · 25,260 dl/mo · 4 contributors · SHA 7c8c79b
- What: The author's personal extension bundle (README states: copy ideas, don't depend on it; breaking changes at will).
- Mechanism (relevant modules): **bwrap** OS sandbox: fs `readonly|workspace-write|allow-all` × network `block|limited|allow-all`; bash tool registers `dangerouslyDisableSandbox` with approval dialog; `/bwrap-*` commands; config `~/.pi/agent/sandbox.json` or `.pi/sandbox.json` (README "bwrap"); **Linux only** (`src/bwrap/runtime.ts:445-452` handles win32 by per-command approval; no darwin branch; README names `apt/pacman/dnf`; `network: limited` needs mihomo + slirp4netns). **spawn-agent**: in-process subagent as a *blocking* tool, defaults to read-only tools (`read/grep/find/ls`), frontmatter `sandbox:` per agent. **talk**: shared SQLite mailbox between pi sessions: `talk-list-agents`, `talk-ask` (blocking, 30 min default), `talk-send`; delivery via `pi.sendMessage` `steer`+`triggerTurn` or `nextTurn` (`src/talk/index.ts:204-208`); pid+start-time presence, bidirectional-ask arbitration by timestamp, 10 s dedup / 30 s rate-limit anti-loop, TypeBox validation of stored data. Also `openai-cost` (cost from response `usage.cost`).
- Maturity: vitest + CI + openspec change records; 3★; bus factor 1 (author-declared instability).
- CPD fit: N3 partial via `talk-ask` (best *design* seen for a blocking Worker→parent question with timeout and anti-deadlock), N16 partial (Linux only, so not usable on the macOS dev machines), N4 partial (default read-only subagent).
- Gaps/risks: macOS unsupported for bwrap; talk presence relies on `/proc/<pid>/stat` for start-time verification (README) → Linux-specific. **Inference:** the SQLite-mailbox design must be reimplemented or the package pinned.
- Verdict: BORROW: copy `talk` design (ask arbitration, delivery modes, anti-loop limits); skip the sandbox on macOS.

### juicesharp/rpiv-mono (npm: @juicesharp/rpiv-ask-user-question, @juicesharp/rpiv-todo; also rpiv-workflow, rpiv-telemetry, rpiv-advisor)
- Meta: ★891 · pushed 2026-10-12 · created 2026-04-18 · MIT · v2.13.8 on all packages (GitHub release list empty) · ask-user-question 299,436 dl/mo, todo 195,262 dl/mo, workflow 4,149 dl/mo · 40 contributors (84 open issues/PRs) · SHA 2d88b79
- What: monorepo of pi extensions by one author; two relevant: a structured-questionnaire tool and a model-owned todo overlay.
- Mechanism: `rpiv-ask-user-question/index.ts` registers the tool (`registerAskUserQuestionTool`, `ask-user-question.ts:377`) and a reconciler. Guards: no UI → returns rejection envelope (`:392 if (!ctx.hasUI) return rejectWithoutUi()`); RPC mode → falls back to `ui.select/input` dialogs (`:412`, `rpc-fallback.ts`); TUI → `ctx.ui.custom` overlay (`:284`). Schema: 1–4 questions, 2–4 options each (label ≤60, description, optional markdown `preview`), `multiSelect`; reserved labels; result envelope; emits event `rpiv:ask-user:prompt` for other extensions to observe (`docs/tool-schema.md`). Emits one BEL when waiting. **Non-interactive runs never see the tool.** `rpiv-todo`: `todo` tool + `/todos`, state stored as full snapshots on the session branch and replayed (survives `/reload`, compaction): a *per-session* todo list, **not** a dependency graph, so not N5. `rpiv-telemetry`: MLflow/console sinks, private, **not published** (npm 404). `rpiv-workflow`: skill-chaining with audited JSONL state, 4k dl/mo, unreviewed in depth.
- Maturity: tests next to every module (`*.test.ts`), biome, i18n (9 locales), changelog, ship-manifest tests; busy repo, same-version lockstep releases.
- CPD fit: N14 full for structured questions (Context-phase interviews), partial for plan approval (compose a `preview`-rich single-select "approve/adjust/reject"). N3 (partial): the tool lets a *Lead* ask the User; a Worker does not need it (CPD Escalation goes to the Lead).
- Gaps/risks: blocks the Lead's turn while open (the User is the only consumer so acceptable); dialog loads from disk at runtime; `pi install` replacing files while pi runs breaks it (README troubleshooting).
- Verdict: REUSE: `@juicesharp/rpiv-ask-user-question` for the Lead.

### nicobailon/pi-interview-tool (npm: pi-interview)
- Meta: ★314 · pushed 2026-09-27 · created 2026-01-02 · license `null` (npm `package.json` author field only; LICENSE absent from `git ls-files`) · v0.13.0 (2026-09-27) · 21,424 dl/mo · 3 contributors · SHA f3eb72f
- What: `interview` tool: local web form (Glimpse WKWebView on macOS, else browser tab) with single/multi-select, text, image upload, charts/mermaid; answers returned to the agent.
- Mechanism: `pi.registerTool` (`index.ts:1031`); params include `async` (`:352`) that returns an interview id immediately and later injects the answer with `pi.sendMessage` (`:1179`); local HTTP server (`server.ts`); queueing for multiple agents (README "Multi-agent behavior"); timeout with countdown; saved snapshots; `session_shutdown` hook (`:1026`).
- Maturity: tests (`index.test.ts`, `schema.test.ts`, `async-interview.test.ts`), CI, changelog.
- CPD fit: N14 partial (richer than rpiv; plan-approval with embedded tables/diagrams is plausible); async mode fits "Lead keeps working while User answers".
- Gaps/risks: no license file; opens windows/browser tabs (focus stealing guarded by queue only); heavier than needed; remote-session handling unverified.
- Verdict: BORROW: consider after v0 if the TUI dialog is too thin; unlicensed, so don't copy code.

### madarco/agentbox and madarco/agentbox-herdr-plugin (npm: @madarco/agentbox)
- Meta: agentbox ★529 · pushed 2026-10-07 · created 2026-05-12 · MIT · `tray-latest` (2026-09-21) · 2,989 dl/mo · 5 contributors · 8 open · SHA 605ee75. agentbox-herdr-plugin ★33 · pushed 2026-06-24 · MIT · mirror of the monorepo's `herdr-plugin.toml`, 0 contributors listed · SHA d92b7bb.
- What: run Claude/Codex/OpenCode/**Pi** in disposable Docker (or Daytona/E2B/Hetzner/Vercel) boxes, each with its own git worktree inside the container; herdr plugin lists boxes and keys.
- Mechanism: pi is a first-class agent package `packages/agent-pi` (`cli/cli-spec.ts`: `agentbox pi`, seed prompt as positional, resume, runs "in a detachable tmux session" inside the box). Host-side relay approval gate for git push/PR/host tools (`docs/architecture.md` lines on `tool.run`, `gh.exec`, `/admin/prompts` SSE + `agentbox agent approvals|approve` for an unattended orchestrator). Herdr bridge: attach wrapper reports box agent state with `pane.report_agent` and shows approval prompts with `notification.show` (`apps/cli/src/terminal/herdr-status.ts:1-30,109-121`); plugin manifest: overlay pane `boxes`, actions, `link_handlers` `agentbox://`.
- Maturity: monorepo with turbo, e2e dir, release/nightly channels, SECURITY.md; large doc set; active.
- CPD fit: N16 partial/full for isolation (container boundary, host-gated pushes) and agent-agnostic (N17) but it is its **own orchestrator** with workspaces/tasks/manager plans (docs `workspaces-tasks-manager-plan.md`), overlapping and likely conflicting with CPD's scheduler. Heavy on an 8 GB M1 (Docker per Worker; **inference**).
- Gaps/risks: replaces herdr panes with `docker exec` attach wrappers (status detection via wrapper); worktree uses bind-mounted host `.git` RW; install weight; metrics/cost not exposed.
- Verdict: SKIP for v0 (post-MVP N16 option; evaluate only if container-level isolation becomes mandatory).

### dcolinmorgan/herdr-remote
- Meta: ★413 · pushed 2026-10-08 · created 2026-06-21 · **AGPL-3.0-or-later or commercial** (GitHub `NOASSERTION`) · v0.8.0 (2026-09-01) · 20 contributors · n/a · SHA bb70162
- What: Python relay + macOS/iOS/Windows apps + Telegram to monitor and approve herdr agents remotely.
- Mechanism: plugin event `pane.agent_status_changed` → `relay/on_event.py` pushes UDP to the relay; relay reads herdr via CLI, sends keys/text with `pane send-keys`/`send-text` (`relay/herdr_relay.py:1746,1876`) to answer prompts; parses transcripts for Claude, **pi** and kimi-code (`relay/transcript.py:11,77-79,440` pi JSONL turn parser, root `~/.pi/agent/sessions`).
- Maturity: tests (`tests/test_herdr_relay.py` etc.), SECURITY.md, token auth, tunnel docs.
- CPD fit: N15/N2 for the *human* only (menu bar/phone alerts, one-tap approvals). No usage metrics (digest = working time + block count).
- Gaps/risks: AGPL; approvals by keystroke injection is fragile; remote exposure surface.
- Verdict: SKIP: AGPL and human-facing.

## Additional finds

### ryonakae/shepherd (npm: @ryonakae/shepherd, @ryonakae/shepherd-pi)
- Meta: ★12 · pushed 2026-08-28 · created 2026-06-24 · MIT · v0.6.0 · 37 dl/mo (shepherd), 55 dl/mo (shepherd-pi) · 1 contributor · SHA 57cf988
- What: Node daemon (SQLite via `node:sqlite`) indexing agent sessions of herdr panes, with a CLI (`shepherd agent list|get|read`) and a pi extension that wakes the "owner" Lead on completed/blocked agents.
- Mechanism: `events.subscribe` for `pane.agent_status_changed` (`src/herdr/socket-client.ts:118`, `src/observability/agent-index-service.ts:243`); pi readers for Claude/Codex/Gemini/OpenCode/Pi (`src/agent-history/pi-reader.ts`); no token/cost fields (grep of `src/`). Pi extension: `/shepherd on` claims owner for the workspace; wake logic `isIdle()` check, deferral until `agent_settled` (`packages/shepherd-pi/src/index.ts:229-260,676`), then two `pi.sendMessage` calls: hidden context (`display:false`, `deliverAs:"followUp"`) and a visible "wake" card (`deliverAs:"followUp", triggerTurn:true`) (`:320-340`).
- CPD fit: **N2 partial (closest design to the need)**, N15 partial (herdr plugin shows agent rows). Not N13.
- Gaps/risks: tiny adoption, one maintainer, 6 weeks stale, no usage data, daemon must run, wakes on any agent in the workspace (not Job-scoped). Whether a half-typed Lead prompt is preserved: unverified.
- Verdict: BORROW: model the Lead-wake design on it; don't depend on it.

### joelhooks/herdr-pings
- Meta: ★9 · pushed 2026-08-09 · created 2026-07-12 · MIT · no releases · SHA b8cea7c
- What: Pi extensions + Bun CLI + herdr plugin producing per-pane "turn settled" events with token counts.
- Mechanism: pi hook `agent_settled` writes `{event:"turn_ended"|"turn_error", pane_id, timestamp, session, turn_index, last_message_tail, error}` to `~/.local/state/herdr-pings/<pane>.jsonl` (`herdr-turn-ping/index.ts:149-181`; token fields only on the optional Convex path, `:170-173`); `herdr-ping-wait <pane_id…> --timeout --cursor` consumes next event (`herdr-ping-wait.ts`, 304 lines); herdr events `pane.exited|closed|agent_status_changed` add `pane_exited/closed/agent_exited_early` records (`herdr-plugin.toml`, `bridge/lifecycle.js`). `herdr-scoreboard` publishes `$task/$turn/$age/NEEDS JOEL` sidebar tokens.
- CPD fit: N2 partial (pull-based, ideal for a deterministic scheduler and crash detection), N13 partial (tokens per turn in Convex path only), N15 partial (sidebar tokens).
- Gaps/risks: 9★, Bun dependency, author-specific bits (Convex, Discworld callsigns, `NEEDS JOEL`).
- Verdict: BORROW: spool+cursor contract and `agent_exited_early` watchdog.

### naturalmoods/herdr-burnlog
- Meta: ★2 · pushed 2026-09-28 · created 2026-09-27 · MIT · no releases · SHA 482a9bc
- What: Python/SQLite collector: persistent token and recorded-cost history by project and model, from Codex, Claude Code and Pi (session format 3) session files.
- Mechanism: `[[startup]]` + `pane.agent_status_changed` → `burnlog.py event` (idempotent scan, `herdr-plugin.toml`); table with `cost_*` and `cost_provenance IN ('recorded','estimated','unavailable')` (`burnlog.py:176-181`); CLI `current|projects|models|collect`; stores at `$XDG_STATE_HOME/herdr-burnlog/burnlog.sqlite3`.
- CPD fit: N13 partial. Granularity project/model/day, not Job/Attempt. Its own README: events carry no usage data; no daemon, missed events need `collect`.
- Verdict: BORROW: schema/idempotent-ingest ideas; CPD's own table keyed by Attempt is simpler.

### @erichll/pi-sandbox (erichll/pi-packages)
- Meta: ★11 · pushed 2026-10-08 · created 2026-07-28 · repo MIT / package Apache-2.0 · 4,085 dl/mo · SHA 6eb4d96
- What: Pi extension sandboxing Bash (and optionally entire subagent process trees) with Anthropic's `sandbox-runtime`: Seatbelt on macOS, bwrap+seccomp on Linux.
- Mechanism: `createBashToolDefinition` with sandbox `operations` (`src/index.ts:254-262`), `user_bash` hook (`:516`), `tool_call` hook (`:531`); static FS policy (writes outside workspace fail closed, secrets write-denied), network static deny→allow→per-connection review by `pi-auto-review` (README "Security model"); provider modes `builtin` (own `subagent` tool; sandboxes whole worker process tree), `pi-subagents` (>=0.76.1; children limited to bash/read/grep/find/ls), `off`; parser rejects unknown config fields (fail closed). Deps `@anthropic-ai/sandbox-runtime ^0.0.78`, `@erichll/pi-auto-review`.
- CPD fit: N16 partial: the only pi-native option seen that covers **macOS**. Applying it to a CPD Worker pane requires either launching `pi` inside this extension's builtin provider (not herdr panes) or relying on the Bash-only protection in each Worker's own pi (file `write/edit` tools remain unsandboxed in that mode: **inference** from README scope "protects main-agent Bash commands").
- Gaps/risks: 11★, pre-1.0 runtime (0.0.78), tied to `pi-auto-review`; interplay with `pi-herdr-subagents` unverified.
- Verdict: BORROW (post-MVP N16 candidate).

### pi-permission-modes (wynainfo/pi-permission-modes) and pi-better-sandbox (1aboveio/pi-better-harness)
- Meta: permission-modes ★14 · pushed 2026-09-24 · MIT · 2,170 dl/mo · SHA 07dae7e. better-sandbox ★10 (monorepo) · pushed 2026-10-11 · MIT · 3,717 dl/mo · SHA 64aeeae (30 open issues).
- What: permission-modes: switchable modes (Default/Plan/Build/YOLO) bundling an OS sandbox profile (bubblewrap/`sandbox-exec`) with tree-sitter-parsed allow/ask/deny. better-sandbox: separate "Main" and "Subagents" sandbox profiles with per-tool trust.
- Mechanism: from READMEs only (source not read): sandbox-runtime for permission-modes; guarded vs trusted tools and detached subagents for better-sandbox.
- CPD fit: N16 candidates; distinct "subagent" profile in better-sandbox mirrors CPD's Worker vs Lead split.
- Verdict: BORROW (not evaluated in depth; unverified claims).

### @pedro_klein/pi-readonly-bash (PedroKlein/pi-extensions) and @ekil9/pi-readonly
- Meta: pedro ★1 · 30 dl/mo · no license · SHA b004ff8; ekil9 17 dl/mo.
- What: `bash_readonly` tool with a command classifier (allows ls/cat/grep/git read/test/build; blocks redirects, `sed`, `$()`, `bash -c`) (README; `src/index.ts:25` registers the tool).
- CPD fit: N4 supplement if the Reviewer needs read-only shell. A denylist/allowlist classifier, not OS enforcement; model can still use the normal `bash` unless excluded via `--tools`.
- Verdict: SKIP for v0; `--tools read,grep,find,ls` is enough.

### ivolkoff/herdr-huddle
- Meta: ★4 · pushed 2026-10-07 · created 2026-10-07 · MIT · no releases · SHA efe97dd
- What: CLI `huddle q "…" "*Yes|reason" "No|reason"` opens a rich question page over the agent's pane (via `terminal-browser`) and prints JSON answers on stdout.
- Mechanism: pane overlay `page` (`herdr-plugin.toml`) running `terminal-browser open $HUDDLE_URL`; blocking CLI; `SKILL.md` for agents. Agent-agnostic (any agent that can run a shell command).
- CPD fit: N14/N17 partial (works for any agent), but the page opens over the *asking agent's own pane*, which contradicts "the User only talks to the Lead".
- Verdict: SKIP.

### Other herdr plugins seen in the index (metadata only, not read)
- `jeffarese/herdr-agent-grid` ★7, v2.0.2, MIT (SHA 67860ba): Rust command center with tokens/cost/time per agent; metrics read from Claude/Codex logs only (`docs/metrics.md`), no pi source listed.
- `DIodide/herdr-telemetry` ★12, v0.4.1, MIT: Go daemon streaming lifecycle events and derived "runs" (working→idle + timing) as NDJSON to a URL (README diagram). Status/timing only, no tokens; `events.subscribe` + `agent.list` diff. Useful if CPD wants wall-clock Attempt duration from herdr rather than from the session file.
- `winoooops/herdr-agent-watcher`, `douglascorrea/herdr-agent-inbox`, `0xGosu/herdr-auto-pilot` (★25, auto-prompts agents), `hkdom/herdr-telegram-gate` (risk-tiered auto-approval): cloned but not read.
- Sandbox plugins (read manifests/README only, none list pi): `vercel-labs/herdr-vercel-sandbox-plugin` (Codex/OpenCode/Claude per persistent Vercel Sandbox, `apply-changes` pulls a git patch), `e2b-dev/herdr-e2b-sandbox` (v0.5.0, worktree snapshot to E2B), `dirien/herdr-sbx-plugin` (Docker Sandboxes microVM; Claude/Codex/Gemini/OpenCode/Copilot/Cursor), `dvdksn/sbx-herdr` (sidebar detection only).
- Messaging/orchestration (index metadata only): `detailles/AgentRadio` (local message bus), `agentwireprotocol/awp`, `jpolec/herdr-plugin-odysseus`, `DnzzL/herdr-docket`, `zhenyufu/herdr-cadence`, `husniadil/herdr-tasks`: relevant to other clusters.

### Pi packages from npm search (not on the list; metadata only unless noted)
- Metrics: `@spences10/pi-telemetry` (repo `spences10/my-pi`, ★135, MIT, 272 dl/mo): **read**: local SQLite `~/.pi/agent/telemetry.db` with `runs`, `turns`, `tool_calls`, `provider_requests` (README "What it records"); hooks `agent_start/end`, `turn_*`, `tool_execution_*`, `before_provider_request`, `after_provider_response` (`src/index.ts:447-582`); disabled by default; Node ≥24.15 for `node:sqlite`. Its README states pi itself owns canonical token/cost accounting and the extension doesn't recompute it. Closest "ready-made per-run SQLite" for N13, but low use and no Job/Attempt keys. Others by downloads: `@tmustier/pi-usage-extension` 8,222; `@amaster.ai/pi-telemetry` 4,193 (Langfuse+OTel, 3.8 MB); `@pi-archimedes/subagent` 2,776 (cost tracking); `@pify/usage` 2,670; `@twkrash/pi-session-inspector` 2,651 (session/subagent analytics); `@po.dev/pi-usage-dashboard` 1,670; `pi-metrics` 1,496; `pi-tokenburn` 1,292 (budgets); `@ayagmar/session-cost-forensics` 600; `observal-pi` 516; `@mammothb/pi-otel` 323; `subagents-pi` 213 (fleet metrics).
- Sandbox/permissions: `@yandy0725/pi-sandbox` 1,732; `@mzwing/pi-permission-auto-review` and `pi-permission-ai-guard` (LLM reviewers layered on gotgenes); `@daytona/pi` (remote Daytona sandbox runs all tool calls); `@stixxert/pi-docker-sandbox`; `@pify/yolo` 2,722 (mode gradient).
- Notify: `@raidou/pi-notify` 1,655; `pi-notify` 941 (OSC 777/99/9); `omp-notify-tool` 29 (model-callable non-blocking `notify` tool, MPL-2.0); `@xyzensun/pi-notify` (ntfy). All notify the *human*; none addresses Lead wake.
- Ask: `@adeildo/pi-ask-questions` 1,068; `@pify/ask-question`, `@henryqw/pi-ask-question`, `@raidou/pi-ask-user`. Same niche as rpiv; none looked better on traction.
- Big adjacent package: `pi-subagents` (`nicobailon/pi-subagents`) 608,963 dl/mo, referenced by `@erichll/pi-sandbox` as the subagent engine; outside this cluster's scope but by far the highest-traffic delegation package seen.

## Gaps left open (unverified)
- Whether a `pi.sendMessage({triggerTurn:true})` wake preserves a half-typed editor buffer in pi 1.1.0 (no code in shepherd/talk touches the editor; pi source not read).
- Whether two extensions that both block in `tool_call` compose (e.g. cc-safety-net + pi-permission-system + sandbox).
- Whether herdr `pane.get` returns `agent_session.path` for pi under the official integration on 0.9.3 (agent-usage/token-dashboard rely on it; I did not run herdr).
- Whether `srt <cmd>` can sandbox a whole pi process with provider-API network access on macOS (guess).
- Plan-approval UI: no cluster item ships one; needs a spike with `ask_user_question` previews or a custom `ctx.ui.custom` component.
