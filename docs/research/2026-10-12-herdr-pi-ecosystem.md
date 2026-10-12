---
date: 2026-10-12
status: research complete; section 8 decisions resolved 2026-10-12 (see requirements §5, ADR-0004 to ADR-0006)
versions: herdr 0.9.3 (source 2563803), pi 1.1.0
---

# herdr plugins and pi extensions: deep research for CPD

**Question.** How do herdr plugins and pi extensions work, how do we build one, what can they do, what already exists for CPD, and what should we reuse, fork, borrow or build?

**Method.**
- **Platform (sections 1–3):** read by the Lead from primary sources: the herdr repo docs and Rust source at `2563803` (`/tmp/herdr-src`), the installed pi 1.1.0 docs, type declarations and dist code, and the installed herdr↔pi integration.
- **Ecosystem census:** the official marketplace index (`https://assets.herdr.dev/plugins/index.json`, 1,685 manifests), 155 manifests fetched at their indexed commit, and the npm registry (`keywords:pi-package`).
- **Deep dives (sections 4–6):** five parallel research Workers (Sonnet, high thinking; specs in [raw/specs/](raw/specs/)). They shallow-cloned and read the source of **~140 repos/packages**, cited `path:line` and the SHA read, installed and ran nothing. Their full reports are the evidence base:
  - [W1 herdr orchestration and task state](raw/w1-herdr-orchestration.md)
  - [W2 pi↔herdr subagents](raw/w2-pi-herdr-subagents.md)
  - [W3 pi orchestration engines](raw/w3-pi-orchestration.md)
  - [W4 review, git, worktrees, PR](raw/w4-review-git-worktree.md)
  - [W5 observability, notifications, safety, user interaction](raw/w5-observability-safety.md)
  - [Top 60 herdr plugins](raw/herdr-index-top60.md)
- **Verification labels.**
  - **verified**: the Lead checked it in source or by running a read-only command.
  - **reported**: a Worker read it in source and cites the location; not re-checked by the Lead.
  - **unverified**: docs or inference only.

---

## 0. Answer first

1. **Nothing in the ecosystem runs CPD end to end.** Every whole-system candidate brings its own model of the world: HenryQW/pi-subagent, pi-herdsman, spoolway, herdr-projects, odysseus, taskplane and mu. None of them combines CPD's mix of:
   - a Locked, append-only Job graph;
   - fresh-session Attempts with a Tier ladder;
   - a Reviewer forked from the Lead;
   - a serialised merge into one Lead Branch;
   - per-Attempt metrics.

   Adopting any of them means bending CPD to it.
2. **The parts do exist, and the hard ones are solved.**
   - Spawning pi Workers in herdr panes with per-spawn model and thinking.
   - Waking the Lead without touching its input box.
   - A read-only forked Reviewer.
   - Worktrees from a chosen base.
   - Per-repo setup, structured questions to the User, and plan approval.
   - The data source for per-Attempt metrics.
3. **What we must build is small and deterministic.**
   - The Job ledger, or a thin wrapper if we use beads.
   - Tier validation.
   - Concurrency cap and timeout.
   - Retry ladder.
   - Merge queue.
   - Metrics collector.
   - PR-feedback poller.
   - The `cpd-lead` skill.

   No candidate provides these in a Job-oriented, herdr-native form. Several provide patterns to copy (section 5).
4. **Four facts change the earlier plan** (`requirements.md` §5):
   - **Escalation.** The `andrewfung729` package is not a fork of `modem-dev`. It launches children by typing commands into a pane and has the only real "ask and wait" tool in that family. Escalation is the weakest link across the subagent packages (W2).
   - **Lead notifications are draft-safe, but only inside pi.** pi's `sendMessage` from an extension never touches the editor (verified, `agent-session.js:1782-1820`). `herdr agent prompt` types into the terminal and does clobber a half-typed draft (W1: measured by herdr-projects on 0.9.1). Rule: anything that wakes the Lead must go through a pi extension, never through `herdr agent prompt`.
   - **`herdr agent start ... -- <args>` passes the args through in 0.9.3** (verified, `src/app/agents.rs:197-199`), so `-- --model github-copilot/claude-sonnet-5.5:low` works. mu's claim that it does not came from herdr 0.8.0.
   - **beads (`bd`) is a strong but heavy store.**
     - Embedded Dolt allows only one writer at a time.
     - The binary is 48 MB.
     - It cannot enforce append-only (it has `delete`, `edit` and `compact`).
     - Ready means the blockers are closed.

     So only the Lead may write to it, and "Merged = closed" becomes a convention (W1).

---

## 1. The herdr plugin platform

### 1.1 Model

A plugin is a directory with a `herdr-plugin.toml` manifest plus commands herdr launches as **argv arrays, not through a shell**. There is no plugin SDK: **the whole herdr CLI is the plugin API**, and plugins call back through `HERDR_BIN_PATH` or the raw socket (`plugins.mdx:1-30`). The plugin owns its language, dependencies, files and state. herdr owns installation, manifest validation, keybindings, panes, events, invocation context and logs.

### 1.2 Manifest entries

| Entry | Purpose | Notes |
|---|---|---|
| top level | `id`, `name`, `version`, `min_herdr_version` (required), `description`, `platforms` | link/install refused if `min_herdr_version` > binary |
| `[[build]]` | commands run at GitHub install, after the trust preview | no runtime env; `plugin link` does not run them |
| `[[startup]]` | runs once per enabled plugin after restore, when the API is ready (also after live handoff) | "one-shot", but plugins start detached daemons from it (`taskr`, `mergr`, `herdr-projects` ticker) |
| `[[actions]]` | invocable commands; `contexts` = `global`, `workspace`, `tab`, `pane`, `selection` (verified `src/api/schema/plugins.rs:355`) | keybinding `type="plugin_action"`; `plugin action invoke` takes no args or stdin |
| `[[events]]` | `on = "<event>"` hook, one process per event | 22 hookable events (1.4) |
| `[[panes]]` | terminal entrypoints; `placement` = `overlay` (default), `popup`, `split`, `tab`, `zoomed` | popups are session-modal and are not panes (no pane ID, no events) |
| `[[link_handlers]]` | Ctrl-click URL regex → action | context gets `clicked_url` |

### 1.3 Runtime context

Every command gets:
- `HERDR_SOCKET_PATH`, `HERDR_BIN_PATH`, `HERDR_ENV=1`;
- `HERDR_PLUGIN_ID`, `_ROOT`, `_CONFIG_DIR`, `_STATE_DIR`;
- `HERDR_PLUGIN_CONTEXT_JSON` (workspace, tab, pane, worktree, agent, selection, URL);
- `HERDR_WORKSPACE_ID`, `HERDR_TAB_ID`, `HERDR_PANE_ID` when available.

Actions also get `HERDR_PLUGIN_ACTION_ID`, event hooks get `HERDR_PLUGIN_EVENT` and `HERDR_PLUGIN_EVENT_JSON`, and panes get `HERDR_PLUGIN_ENTRYPOINT_ID` (`socket-api.mdx:447-606`). Process-launching CLI commands (`pane split`, `tab create`, `workspace create`, `plugin pane open`) accept repeated `--env KEY=VALUE` (`cli-reference.mdx`).

### 1.4 Hookable events (verified, `src/api/schema/events.rs` `PLUGIN_HOOK_EVENT_KINDS`)

| Group | Events |
|---|---|
| workspace | `created`, `updated`, `closed`, `renamed`, `moved`, `reordered`, `focused` |
| worktree | `created`, `opened`, `removed` |
| tab | `created`, `closed`, `renamed`, `moved`, `focused` |
| pane | `created`, `closed`, `focused`, `moved`, `exited`, `agent_detected`, `agent_status_changed` |

High-volume events are deliberately excluded from hooks: `pane.output_changed`, `pane.updated`, `layout.updated`, `workspace.metadata_updated`. Socket subscribers still get them, plus `pane.output_matched`, `pane.scroll_changed` and others. An unknown `on` name links with a warning; it does not fail.

### 1.5 Socket API features that matter for CPD

- **`events.subscribe`.**
  - Ordered within a subscription and not durable.
  - A slow reader gets `events_lost`, then must resubscribe and reconcile via `session.snapshot` (`socket-api.mdx:771-840`).
  - Subscribing to `pane.updated` is expensive (herdr-radar measured ~110 ms per write vs 1–2 ms, W5).
- **`agent.view.set`.** A declarative filter and sort for the sidebar Agents view, with filters on status, workspace, tab, pane, agent, seen and **plugin metadata tokens** (`socket-api.mdx:368-446`). CPD can tag Worker panes with a token and filter them out, so the User's sidebar shows only the Lead (N15). It is raw-socket only; there is no CLI.
- **`pane.report_metadata` / `workspace.report_metadata`.** Display-only tokens (`$name` in sidebar rows), with TTL and `seq`. They do not affect state or waits.
- **`pane.report_agent` / `report_agent_session`.** Custom agent state plus `resume_argv` (0.9.2+). This is how any agent "adds herdr support".
- **`worktree.create/open/list/remove`.** `create` runs `git worktree add -b <branch> <path> <base>`. An existing local branch is checked out and `--base` ignored (W4, `src/worktree.rs:239-258`). Never deletes branches.
- **`agent.start`.** Types `<kind executable> <args…>` into a pane whose shell is at its prompt, then waits for detection and readiness (verified, `src/app/agents.rs:145-230`). Supported kinds include `pi`, `claude`, `codex`, `hermes`, `opencode`.
- **`agent.prompt` with `wait`.** Submit and wait in one request. It submits by typing into the target terminal (see 3.3).
- **`plugin.pane.open`.** Launches an argv-backed pane with `--target-pane`, `--direction`, `--cwd`, `--env` and `--no-focus`. It needs no shell, so there is no typing race (used by modem-dev, 3.1).
- **`layout.export` / `layout.apply`, `session.snapshot`.** Bootstrap and restore layouts.

### 1.6 Limits (what plugins cannot do)

- **No runtime registration.** Actions, panes and hooks are manifest-only. No native (non-terminal) UI. No storage API: the config and state dirs are path discovery only.
- **Plugin commands have no timeout.** At most **32 in flight** (`plugin_command_limit_reached`), and the logs keep the last 200 (verified, `src/app/api/plugins/runtime.rs:12-13,264-300`). A daemon must manage its own lifecycle (single instance, restart).
- **Ordering.** Event hooks are separate processes, not ordered relative to the caller. Assume `worktree create` returns before `worktree.created` hooks finish (W4 inference: `tdi/herdr-worktree-setup` publishes a `setup: running/done` token for exactly this reason).
- **Distribution and trust.** Installs are GitHub-only (`owner/repo[/subdir]`, `--ref` pins) and global across all sessions. There is no sandbox: plugins run as the user.

### 1.7 How to build one

```toml
# herdr-plugin.toml
id = "cpd"
name = "CPD"
version = "0.1.0"
min_herdr_version = "0.9.3"
platforms = ["macos", "linux"]

[[startup]]            # optional: start/ensure a single-instance daemon
command = ["sh", "bin/ensure-daemon.sh"]

[[actions]]
id = "status"
title = "CPD status"
contexts = ["workspace"]
command = ["sh", "bin/status.sh"]

[[events]]
on = "pane.agent_status_changed"
command = ["sh", "bin/on-status.sh"]   # reads $HERDR_PLUGIN_EVENT_JSON

[[panes]]
id = "board"
title = "CPD board"
placement = "split"
command = ["sh", "bin/board.sh"]
```

- **Dev loop:** `herdr plugin link <dir>`, then `herdr plugin action invoke cpd.status`, `herdr plugin log list --plugin cpd`, `herdr plugin pane open --plugin cpd --entrypoint board`.
- **Patterns worth copying:**
  - **Test harness with a fake `herdr` binary:** `herdr-spreader` `tests/fixtures/fake-herdr.sh`, `herdr-board` `client/fake.rs`, modem-dev's real-pi harness.
  - **Socket client:** typed in bellwether `src/herdr-client.ts`; minimal in reviewr `src/herdr_socket.rs`.
  - **Single-instance daemon from `[[startup]]`:** taskr `plugin/run.sh`.
  - **CLI quirks list:** `herdr-projects` `docs/herdr-notes.md`. It documents ids being reused after a server restart and the trust dialog that makes `agent start` fail fast with `agent_not_ready`.

### 1.8 Ecosystem census (verified from the index, 2026-10-12)

- **Size and growth.** 1,685 manifests in 1,630 repos. Repos created per month: Jun 66, Jul 382, Aug 451, Sep 501, Oct (12 days) 214.
- **Stars.** 34 repos ≥100★, 154 ≥10★, 963 ≥1★.
- **Languages.** Rust 361, Python 336, Shell 330, JS 220, Go 181, TS 150.
- **Usage across the 155 manifests with ≥10★.**
  - Entries: actions 145, panes 115, build 96, events 51, startup 35, link handlers 8.
  - Most hooked events: `pane.agent_status_changed` 22, `pane.agent_detected` 17, `workspace.created` 16, `pane.focused` 15, `worktree.created` 11.
  - Pane placements: split 69, overlay 62, popup 59, zoomed 18, tab 9.
  - First argv: `sh`/`bash` 79, `cargo` 19, `node` 18.
- **Rough topic buckets (regex over descriptions; overlapping):** layout/navigation ~1,010, observability ~518, worktree/git ~299, workflow/task ~272, review ~166, notify/remote ~167, orchestration ~141, safety ~69.

---

## 2. The pi extension platform

### 2.1 Model

An extension is a TypeScript module (loaded by `jiti`, no build step) whose default factory receives `ExtensionAPI`. It runs **in-process with pi's permissions** (`extensions.md`). Long-lived resources (sockets, watchers, timers) start in `session_start` and close in an idempotent `session_shutdown`, never in the factory. Extensions load in TUI, RPC, JSON and print modes. `ctx.mode` and `ctx.hasUI` gate UI-only behaviour.

### 2.2 Capabilities (verified, `dist/core/extensions/types.d.ts`)

| Capability | API |
|---|---|
| Lifecycle and data hooks | `pi.on(event)`, ~40 events: `session_*`, `before_agent_start`, `agent_start/end`, `agent_before_settle`, **`agent_settled`** (final, "pi will not continue"), `turn_*`, `message_*`, `tool_call` (can mutate or **block**), `tool_result`, `input`, `user_bash`, `context`, `before_provider_request`, `provider_stream_event`, `model_select`, `thinking_level_select`, … |
| Model-callable tools | `registerTool` with TypeBox params, `outputSchema`, `exposure` (`direct`, `model-only`, `codemode`, `deferred`, `hidden`), MCP-style `annotations` (`readOnlyHint`…), `ctx.executeTool` for nested calls |
| Commands, shortcuts, CLI flags | `registerCommand`, `registerShortcut`, `registerFlag` |
| Inject into the conversation | `sendMessage(custom, {triggerTurn, deliverAs: "steer"\|"followUp"\|"nextTurn"})`, `sendUserMessage(text, {deliverAs})` |
| Persist outside context | `appendEntry` (session JSONL) |
| Session control | `setModel`, `setThinkingLevel`, `setActiveTools`, `getAllTools`, `exec` |
| Cross-extension bus | `pi.events` (e.g. `herdr:blocked`, `plannotator:request`, `subagents:rpc:*`) |
| Providers, MCP, virtual models | `registerProvider`, `registerMcpServer`, `registerVirtualModel` |
| UI | `ctx.ui` notify/confirm/select/input/custom components, widgets, status, renderers |

### 2.3 Non-extension surfaces that CPD can use directly (verified in `pi --help` and docs)

- **Read-only forked Reviewer with no extension:** `pi --fork <session>` plus `--tools read,grep,find,ls`. `--tools` is a strict allowlist that replaces the default tools (W2).
- **Tier at launch:** `--model <provider>/<id>:<level>` or `--thinking <level>`. Unsupported levels are clamped silently (requirements §3).
- **Non-interactive modes.**
  - `-p` print mode.
  - `--mode json`: an event stream including usage. The official subagent example parses it (`examples/extensions/subagent/index.ts:300-376`).
  - `--mode rpc`: JSON over stdin/stdout, including `get_session_stats` with tokens, cost and message counts (W5).
- **SDK.** In-process `createAgentSession`, with session `fork()` and steer/followUp (`sdk.md`). This is how in-process engines (nicobailon, tintinweb) work.
- **Session JSONL.**
  - Header: version 3 with `cwd` and `parentSession` for forks.
  - Every assistant message carries `usage{input, output, cacheRead, cacheWrite, totalTokens, cost{…total}}`, model, provider, `thinkingLevel`, timestamps and `durationMs` (verified docs; W5 read a live file).
  - **This is the complete N13 data source.**
- **Codemode.** A QuickJS sandbox in which a script calls tools in parallel (no network or filesystem except through tools).

### 2.4 Packages and distribution

- **Install:** `pi install npm:<pkg>@<ver>` (pinned), `git:<url>@<ref>`, or a local path. `pi -e <source>` tries one for a single run. Personal installs go to `~/.pi/agent/settings.json`; project installs to `.pi/settings.json` (requires project trust).
- **Filtering:** per-resource filters in settings (`"extensions": ["!x"]`, `"skills": []`).
- **Dependencies:** host packages (`@earendil-works/*`, `typebox`) must be `peerDependencies: "*"`, never bundled.
- **Gallery:** the `pi-package` npm keyword makes a package listable on pi.dev. **11,799** npm packages carry it (verified, npm search total).

### 2.5 The herdr↔pi glue already installed here (verified, `~/.pi/agent/extensions/herdr-agent-state.ts`, integration v9)

- Reports `working`, `idle` and `blocked` and the session path to herdr **only in TUI mode**. Headless `pi -p` Workers are invisible to herdr state.
- **Any extension can mark the pane blocked** by emitting `pi.events.emit("herdr:blocked", {active, label})`. This is a ready-made way to surface an Escalation as `blocked` in herdr.
- `herdr pane get <pane>` returns `agent_session {kind: "path", value: <session.jsonl>}` for pi panes (verified live on this pane). A metrics collector can therefore go from Worker pane to session file to usage with no extra reporting.

### 2.6 How to build one

```text
cpd-pi/
├── package.json        # "keywords": ["pi-package"], "pi": {"extensions": ["./extensions/cpd.ts"], "skills": ["./skills"]}
├── extensions/cpd.ts   # tools (cpd_job_*), agent_settled hook, Lead wake via sendMessage
└── skills/cpd-lead/SKILL.md
```

Dev loop: `pi -e ./extensions/cpd.ts`, then `/reload`; `pi install ./cpd-pi` for a durable install. The official `examples/extensions/` are the reference: `subagent/`, `question.ts`, `questionnaire.ts`, `send-user-message.ts`, `permission-gate.ts`, `git-checkpoint.ts`.

---

## 3. Integration patterns the ecosystem converged on

### 3.1 Launching a pi Worker in a herdr pane

| Pattern | Who | Race-free? | Notes |
|---|---|---|---|
| `herdr plugin pane open … --env SCRIPT=…`, dispatcher `exec`s a generated `pi …` command line | modem-dev | **yes, by construction** | needs the bundled herdr plugin linked |
| `herdr agent start <name> --kind pi --pane <id> -- <args>` | herdsman, tobi, AndrewJacop, fyang93, HenryQW | mostly | herdr types into a shell it has checked is at its prompt, then waits for detection (verified source); slow direnv/devenv shell init is the residual risk (modem-dev README) |
| `herdr pane split` + `pane run "pi …"` | giuseppecrj (waits for shell via `pane process-info`), edxeth, andrewfung729, kirel | open-loop | typed; mitigated by readiness polling or a delay |

### 3.2 Detecting completion

- `.exit` sidecar written by a child extension, then a poll or `fs.watch`. This is the most common pattern.
- Socket `pane.exited`/`pane.closed` (modem-dev).
- `agent_settled` written to a per-pane JSONL spool and consumed with a cursor (joelhooks/herdr-pings). This is the best fit for a deterministic scheduler.
- herdr `agent wait` tracks lifecycle, not turns: "it does not track individual turns" (`agent-automation.mdx`).

### 3.3 Delivering to the Lead without clobbering the User's draft

- **Draft-safe (verified):** `pi.sendMessage`/`sendUserMessage` from an extension in the Lead process. It appends, steers or follows up inside the agent session and never touches the editor (`agent-session.js:1782-1820`). The interactive editor is only cleared on submit (W2, `interactive-mode.js`).
- **Not draft-safe:**
  - `herdr agent prompt` and `pane run`/`send-text` type into the terminal.
  - Packages that route child→parent messages this way inherit the hazard: AndrewJacop `herdr_message_agent`→orchestrator, fyang93 `swarm_send`, tobi's non-pi fallback.
  - herdr-projects measured the merge-with-draft problem on 0.9.1 and built a screen-scraping "input box empty for 10 s" guard (W1).
- **Wake policy patterns worth copying:**
  - nicobailon `parent-wake.ts`: append with `triggerTurn:false`, then one steer wake, a shared `Symbol.for("pi.idle-wake.v1")` reservation so two extensions do not double-wake, and wakes held after a user abort.
  - bellwether: coalesce wakes until `agent_end`, then one follow-up. It measured 124 of 217 wakes as no-ops without coalescing.
  - fyang93: deliver only when idle, at `agent_before_settle`.

### 3.4 Escalation (Worker → Lead question, then wait)

| Mechanism | Who | Child stays alive? | Notes |
|---|---|---|---|
| `caller_ping`: exit, then the parent `subagent_resume`s | modem-dev, HazAT, edxeth, giuseppecrj | no (new pane, same session) | giuseppecrj **rejects resume for worktree children** |
| `ask_question`: park as `waiting`, reply typed into the child pane | andrewfung729 | yes | reply path types into the *child* (fine) |
| `ask_owner`/`reply_agent` durable mailbox, correlated IDs | pi-herdsman | yes | strongest protocol; per-level, not auto-forwarded |
| `intercom ask`: local broker, blocking, 10-min default | nicobailon/pi-intercom | yes | separate `intercom` namespace, coexists with any spawner; herdr-aware |
| `contact_supervisor` | nicobailon/pi-subagents | yes | only for that package's own children |

### 3.5 Reviewer forked from the Lead (N4)

`pi --fork <lead-session> --tools read,grep,find,ls` is upstream-native. Spawners expose it as `fork:true` or `session-mode: fork` (modem-dev, giuseppecrj, edxeth, AndrewJacop). **Every bundled "reviewer" agent ships `read,bash`, which is not read-only**, so CPD must define its own (W2). pi-goal-x's auditor is read-only by instruction only and gets `bash` (W3).

### 3.6 Metrics (N13)

Every serious tool derives metrics from the pi session JSONL:
- spoolway's `usage.jsonl` ledger;
- herdr-agent-usage `omp/extract.go`;
- herdr-token-dashboard `readPiSession`;
- herdr-burnlog (SQLite, idempotent, triggered by `pane.agent_status_changed`).

None keys the data by Job, Attempt or Tier. Writing our own collector is about 50 lines (W5).

---

## 4. Per-need map: reuse, borrow or build

**REUSE** = depend on it as-is (pinned). **BORROW** = copy a pattern or code, with licence permitting. **BUILD** = ours.

| Need | Reuse | Borrow (from) | Build | Confidence |
|---|---|---|---|---|
| N1 spawn pi Worker, per-spawn model + thinking | **giuseppecrj/pi-herdr-agents** (MIT, CI, 38 test files, `model`+`thinking`+`cwd`+`fork`+`tools`+`worktree{branch,base}`) or **modem-dev/pi-herdr-subagents** (argv launch, socket lifecycle; **no LICENSE file**) | AndrewJacop / edxeth: reject invalid thinking instead of clamping | Tier → `provider/id:level` mapping + validation | high (reported); spike to confirm |
| N2 wake Lead, draft-safe | the spawner's own `sendMessage` delivery | nicobailon `parent-wake.ts`, bellwether coalescing, herdr-pings spool | Lead-side wake policy for CPD events | **high (verified mechanism)** |
| N3 escalation | **nicobailon/pi-intercom** (`ask`, MIT, 534★) | herdsman `ask_owner` protocol, andrewfung729 `ask_question` | escalation rules in the skill; `herdr:blocked` emit | medium (no runtime test) |
| N4 read-only forked Reviewer | pi built-in `--fork` + `--tools` (via the spawner's fork option) | n/a | `reviewer` agent definition | high |
| N5 Job graph, history | **beads** `bd` (Lead-only writer), or none | mu (SQLite DAG, append-only ops log), herdr-dagr `CONTRACT.md` (Task/Attempt/cause/evidence) | thin SQLite ledger (if not beads) | medium; decision needed |
| N6 concurrency cap per machine | none machine-wide | herdr-board `dispatch_pass` (claim before launch), AndrewJacop queue, pi-cowboy per-model limits | cross-process cap (file lock or SQLite counter) | high that we must build |
| N7 timeout per Attempt | none herdr-native | sean1588 three layers (no-progress, blocked, hard deadline), edxeth `timeout`/`idle-timeout` | timer in the scheduler | high that we must build |
| N8 retry ladder | none | @quintinshaw `gate(thunk, validator, attempts)`, spoolway `loop:` budgets | ladder in the scheduler/skill | high that we must build |
| N9 worktree from Lead Branch HEAD + setup | herdr `worktree create --base` (or the spawner's `worktree` param); **lamngockhuong/herdr-worktree-setup** (per-repo `.herdr-worktree.toml`, trust file) **or** worktrunk `wt switch --create` with blocking `pre-start` hooks | pi-cowboy copy-on-write `node_modules`/`.env` seeding, jlimas CoW seeding | "wait for setup, then start Worker" ordering | medium (setup race) |
| N10 merge into Lead Branch, commits kept, serialised | **worktrunk** `wt merge <lead> --branch <job> --no-squash --no-remove` (CAS ref update, fails cleanly on races) | herdr-swarm `harvest-step-merge.sh` (`merge --no-ff` + CAS `update-ref` + journal), taskplane `merge.ts` | per-Workflow merge lock and queue | medium; semantics decision needed |
| N11 Gate | worktrunk `[[pre-merge]]` hook running the repo `verify` | pi-subagents/tintinweb `gate` | run `verify` in worktree and on the Lead Branch after merge | high |
| N12 PR feedback loop | none | odysseus `gh pr view --json statusCheckRollup,reviews,comments` + inline comments (`src/github/mod.rs:161-215`); bellwether `herdr_watch` on a `gh pr checks --watch` pane; mergr startup-daemon poller | poller that appends Jobs | high that we must build |
| N13 metrics | none keyed by Attempt | herdr-agent-usage / token-dashboard parsers, spoolway ledger | collector: pane → `agent_session.path` → sum usage | **high (verified data path)** |
| N14 structured questions, plan approval | **@juicesharp/rpiv-ask-user-question** (299k dl/mo; TUI/RPC; 1–4 questions, markdown preview); optional **@plannotator/pi-extension** for browser plan review (event API, plan mode off) | pi-goal-x questionnaire | plan rendering for approval | medium (spike the approval UX) |
| N15 visibility | optional: herdr-radar or herdr-bar; herdr-dagr pane later | herdr `agent.view.set` token filter (hide Workers) | Lead status via `report-metadata` | high (feature verified in docs) |
| N16 safety (post-MVP) | **cc-safety-net** (pi `tool_call` blocklist, 1.6k★) as first step; **@erichll/pi-sandbox** (Seatbelt on macOS) later | gotgenes/pi-permission-system per-role policy | n/a | medium |
| N17 agent-agnostic (later) | herdr `agent start --kind claude\|codex\|hermes`; **herdr-projects** as the coordinator to evaluate | asermax/herdr-subagents (one source → pi package + Claude plugin), ExaDev/agent-comms (bridges) | adapter seam | low (future) |
| N18 cheap aux agents | any per-spawn model (`H0–H3`) | pi-advisor direct model calls (no session) | Tier policy for aux tasks | high |
| N19 building blocks | n/a | sections 1.7, 2.6, 3.x | n/a | n/a |

---

## 5. Shortlist profiles

Facts come from the Worker reports (reported) unless marked otherwise. ★ = stars, dl = npm downloads/month, on 2026-10-12.

**Spawning and lifecycle**
- **giuseppecrj/pi-herdr-agents** (★49, 5.8k dl, MIT, v3.2.1, 5 contributors, CI, 38 test files).
  - One `subagent` call takes `model`, `thinking`, `cwd`, `fork`, `tools` and `worktree:{branch, base}`, the last resolved to an exact commit and created via `herdr worktree create`.
  - Completion via `.exit` plus `fs.watch`; draft-safe steer delivery.
  - Gaps: typed launch (it waits for the shell), no cap or timeout, `caller_ping` escalation, resume rejected for worktree children, 63k lines of code.
  - **Primary REUSE candidate.**
- **modem-dev/pi-herdr-subagents** (★39, 1 author, no releases, no CI, **no LICENSE file**).
  - Argv launch through its own herdr plugin pane, socket lifecycle, honest crash classification, fork plus tool allowlist, 18 test files.
  - Gaps: no worktree, cap or timeout; old peer imports.
  - **Most robust launch.** Cannot be forked safely until it has a licence; using it as-is is a licence question.
- **nicobailon/pi-subagents** (★3.9k, 609k dl, v0.77).
  - The ecosystem's most-used delegation layer: in-process or detached SDK sessions, scripted workflows, supervisor channel, worktrees as patches, FleetView.
  - It does **not** spawn local Workers into herdr panes (herdr placement only for saved remote machines), and its worktrees return patches, not commits.
  - **BORROW** (wake, supervisor, fork semantics).
- **pi-herdsman** (★178, 9.2k dl, Apache-2.0, tested on exactly pi 1.1.0 + herdr 0.9.3). A Manager→Lead→Agent tree with `herdr agent start`, a durable mailbox and the best `ask_owner`. Model and thinking only per definition, no fork, no per-Agent worktree. **BORROW**; a whole-system alternative.
- **HenryQW/pi-subagent** (24.6k dl, pi ≥1.1 / herdr ≥0.9.3). Checked task graphs with `dependsOn`, worktrees pinned to Main HEAD, stage/validate/promote. But only 4 fixed model classes and a hard-wired `pnpm test`. **BORROW** (merge and recovery design).

**Job graph and state**
- **beads `bd`** (★27.8k, MIT, v1.3.1, daily releases). It provides:
  - `bd ready`, atomic `--claim`, dependency types, Dolt history plus `bd history`, gates for `gh:pr`/`gh:run`, epics and cross-repo dependencies (post-MVP).
  - Constraints: single writer (Lead only), 48 MB binary, append-only not enforceable, "closed" ≠ Merged unless we make it so.
  - Viewers exist: **herdr-beads** (REUSE for N15), **beady-eye** (`bdi gates` settle PR/CI events; the wake is Claude-only).
- **mu** (★4, 4.7k dl). The closest design relative: SQLite DAG with a single `blocks` edge, substates (`superseded`, `rejected`…), an append-only ops log with `mu undo`, a per-agent pi control socket. Not mature enough to depend on. **BORROW** the schema.
- **herdr-dagr** (★96, Apache-2.0). A viewer only, but its `CONTRACT.md` vocabulary matches CPD:
  - Task state projected from Attempts.
  - Attempt `cause` = `sent_back`, `gate_failed`, `followup`, `superseded`.
  - Evidence tiers `verified`, `reported`, `heuristic`, `asserted`.
  - Append-only events, and a `dagr check --strict` linter.

  **BORROW** the vocabulary now; REUSE the pane later.

**Git, worktrees, merge**
- **worktrunk `wt`** (★9.2k, MIT/Apache, v0.80). Covers create, setup and merge:
  - `switch --create --base` with blocking `pre-start` hooks, which solves the setup race;
  - `copy-ignored` for env files;
  - `merge --no-squash`: commit, rebase, `pre-merge` hooks, then a fast-forward with a compare-and-swap ref update.

  Not installed here. **REUSE candidate** (spike). The herdr wrapper plugin is interactive-only, so skip it.
- **lamngockhuong/herdr-worktree-setup** (★7, MIT, CI). Per-repo `.herdr-worktree.toml`, copies env files, `post_create`/`post_remove` gated by a trust file. Completion signal is only a toast. **REUSE candidate** if we create worktrees through herdr.

**User interaction**
- **@juicesharp/rpiv-ask-user-question** (★891 monorepo, 299k dl). Structured TUI/RPC questions with previews. **REUSE** for the Context interview.
- **@plannotator/pi-extension** (★9.4k, 134k dl). Browser plan review and annotation. Decisions come back as a follow-up, so it is non-blocking. Heavy. Its plan mode changes the model and tools, so use only the event API. **Optional REUSE** for plan approval.

**Future / agent-agnostic**
- **herdr-projects** (★608, MIT, v0.2.34). Mature:
  - a coordinator that never does the work;
  - threads in worktrees on `--base`;
  - profiles ≈ Tiers for 24 agent kinds;
  - a PR follow-up routine;
  - the best documentation of herdr pitfalls.

  No Job dependencies, one PR per thread, single maintainer, high churn. **Evaluate when Claude/Hermes Workers arrive.**
- **spoolway** (★8, MIT). Model-free dispatcher with pipelines and a usage ledger read from pi transcripts. Never merges; pi effort not wired. **BORROW** (ledger, loop budgets).

**Set aside** (reasons in the reports):
- Linear YAML runner: herdr-workflows.
- No dependency fan-in, no licence: herdr-board.
- Race-and-pick-winner model: herdr-swarm.
- One issue → one squashed PR: sean1588/herdr-orchestrator.
- Scripts the Lead must write and that occupy its conversation: the pi workflow engines (pi-workflows, pi-extensible-workflows, pi-dynamic-workflows).
- Claude/Codex-only: zoetrope, termaxa, herdr-agent-grid, herdmates.
- AGPL: herdr-remote.
- Human-only review UIs: reviewr, hunk-diff, annotate. Good for the User's own PR review, not the Dispatch loop.

---

## 6. Proposed v0 composition (draft, for decision)

```text
User ⇄ Lead (pi, O3, herdr pane)
        ├─ skills: cpd-lead (Context/Plan/Dispatch protocol), grilling
        ├─ pi packages: spawner (giuseppecrj OR modem-dev), pi-intercom, rpiv-ask-user-question
        ├─ cpd extension (ours): Job/Attempt tools over the ledger, Tier validation,
        │     scheduler (cap, timeout, ladder), wake policy (sendMessage only),
        │     metrics collector, herdr:blocked on escalation
        └─ cpd CLI (ours, deterministic): merge queue (wt merge under lock), verify,
              PR poller (gh), ledger I/O  ← agent-agnostic seam for Claude/Hermes later
Workers  = fresh pi sessions in herdr panes on per-Attempt worktrees (base = Lead Branch HEAD)
Reviewer = pi --fork <lead> --tools read,grep,find,ls (via spawner fork option)
Ledger   = beads (Lead-only writer) OR thin SQLite (mu/dagr schema)
herdr    = panes, worktrees, sidebar (agent.view.set hides Workers), notifications
```

Estimated custom code: a pi extension plus a CLI of a few hundred to ~1–2k lines, mostly glue (guess, not measured). The deterministic parts live in the CLI so that a non-pi Lead can drive them later.

---

## 7. Risks and open unknowns

- **Unverified at runtime.** Nothing from the ecosystem was installed or run. Each REUSE candidate needs a spike before design lock.
- **Licences.** modem-dev, tdi/*, herdr-board, yankewei/focus-notify and pi-interview have no licence file. Without one we may use them but not fork or copy them. herdr-remote is AGPL.
- **Single maintainers and churn.** Most candidates are 1–3 months old with weekly releases and one maintainer. Pin versions, and prefer packages with CI and tests.
- **Tool-name collisions.** The whole `subagent` family registers the same tool names, and the first one loaded wins silently. **Install exactly one spawner.**
- **Setup race.** herdr event hooks are asynchronous. A Worker must not start before setup finishes.
- **Memory on the MacBook Air (8 GB).** Language-server add-ons like pi-lens hold a warm server per session. Keep Workers lean.
- **Lead context.** Wakes and results land in the Lead conversation. Wake coalescing and short result envelopes matter.

---

## 8. Decisions needed (questions for the User)

> Resolved 2026-10-12: 1 by trial; 2 beads; 3 rebase + fast-forward (trunk-based); 4 `cpd` CLI plus thin pi extension; 5 worktrunk for v0; 6 plannotator; 7 yes for v0, replace with `cpd`-owned code over time. Recorded in `requirements.md` §5 and ADR-0004 to ADR-0006.

1. **Spawner:** giuseppecrj (worktree param, MIT, typed launch) or modem-dev (argv launch, no licence file)? Proposal: spike both on one repo; default to giuseppecrj.
2. **Job ledger:** beads (rich, 48 MB, Lead-only writer, conventions for Locked/Merged) or a thin SQLite ledger with mu/dagr's schema (enforceable append-only via triggers; the original plan)?
3. **Merge semantics** ("keep commits"):
   - (a) rebase, then fast-forward (worktrunk default with `--no-squash`): linear history, commits rewritten with new SHAs;
   - (b) `--no-ff` merge commits: original SHAs, merge bubbles.
4. **Control-plane location:** pi extension only (simplest for v0), or a `cpd` CLI for the deterministic parts plus a thin pi extension (agent-agnostic seam now)? Proposal: the CLI plus a thin extension.
5. **Worktree and setup tool:** herdr `worktree create` plus herdr-worktree-setup (hooks asynchronous; we wait), or worktrunk end to end (blocking setup hooks, then `herdr worktree open` to register)?
6. **Plan approval UI:** TUI `ask_user_question` with a markdown preview, or plannotator's browser review?
7. **Licence policy:** may we depend on unlicensed repos as-is, or only on licensed ones?
