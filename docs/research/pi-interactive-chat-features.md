# Interactive chat features of pi-agent and cpd's pi extensions: inventory, durable gaps, coupling and measured use

Answers decision pro-d8 (User, 2026-11-03 EST; `read` in the Worker spec `specs/report-docs-a1.md`) for cpd-webui and the pi-agent vs pi-durable choice. This report is a synthesis of five explore artefacts and re-measures nothing. It recommends no web UI base (`read`, spec out of scope).

Evidence labels used on every claim line: `read` (a path:line or doc heading was opened), `measured` (a script run, with the artefact table), `inferred` or `guess` (not verified). Artefact short names: `inv-pi`, `inv-cpd`, `gap-durable`, `use-prompts`, `use-features` (full paths in section 2).

## 1. Summary

- **Q1 Inventory** (`read`): 131 features in 87 pi-agent baseline rows (inv-pi) and 44 cpd-extension rows (inv-cpd), section 3. Under `pi --mode rpc` the cpd adapter is inert: `start()` returns at `ctx.mode !== "tui"` (`pi/lib/startup.ts:32`, spot-checked on origin/main), so only `ask_user_question` keeps an rpc fallback (`pi/ask-user-question/rpc-fallback.ts`).
- **Q2 What raw pi-durable and pi/durable-host lack** (`read`): raw pi-durable has no commands, dialogs, UI API, footer or tree navigation (gap-durable 1 preface, keyword searches `measured`). The host marks over the 131 rows: 22 ok, 13 stream-only, 31 degraded, 26 missing, 39 n/a (section 4). The wire carries dialogs, commands and `host_state` but no tool-row, thinking, diff or footer kinds (`pi/durable-host/protocol.ts:114` `KINDS`, spot-checked).
- **Q2 top defects** (`read`): G4 label check sees no chat (`run.ts:231`), `configure` refused while a run is pending (`run.ts:469`), `fork` ignores `at` (`run.ts:495-500`), cpd compaction ratio unwired, no MCP attach. All four spot-checked on origin/main 2cbb3b71f.
- **Q3 Coupling** (`inferred` classification over `read` tags): 37 `tui-hard`, 18 `rpc`, 35 `durable-wire`, 41 `small-addition` (section 5). The tui-hard rows are keyboard, chrome and TUI-patch features that a browser replaces rather than ports.
- **Q4 Empirical use** (`measured`): the interactive corpus is 242 sessions at the use-prompts cutoff and 266 loaded (269 db rows) in use-features, section 6. The User typed 749 prompts, with 0 persisted slash commands, 0 `!bash`, 0 image blocks, 0 real `@file` mentions.
- **Q4 Dialogs** (`measured`): `ask_user_question` 703 calls, 122 (17.4%) refused before the dialog; `cpd_plan_review` 301 calls, 240 APPROVED, 0 REJECTED; `cpd_secret_approve` and `cpd_host_approve` 0 calls; 286 compactions in 96 sessions.
- **Priority** (`inferred` from the measured numbers): P0 is prompt submit, streaming text, tool rows with truncation, `ask_user_question`, plan-review Approve, history and cpd message folds; P1 is abort, compaction, thinking fold, footer, MCP; P2 is everything the User never used (section 7).

## 2. Method and corpus

### 2.1 Inputs and spot-checks

| Input | Path | Ev |
|---|---|---|
| inv-pi | `/Users/Clayton.Duarte/.cpd/run/pi-chat-features/explore/inv-pi.md` (pi-coding-agent 1.3.0 + pi-tui, rpc exposure) | read |
| inv-cpd | `/Users/Clayton.Duarte/.cpd/run/pi-chat-features/explore/inv-cpd.md` (cpd `pi/` at origin/main 2cbb3b71) | read |
| gap-durable | `/Users/Clayton.Duarte/.cpd/run/pi-chat-features/explore/gap-durable.md` (pi-durable 1.3.0 and `pi/durable-host`) | read |
| use-prompts | `/Users/Clayton.Duarte/.cpd/run/pi-chat-features/explore/use-prompts.md` (script `/tmp/pi-chat-features/prompts.py`) | read |
| use-features | `/Users/Clayton.Duarte/.cpd/run/pi-chat-features/explore/use-features.md` (scripts `/tmp/pi-chat-features/features.py`, `mkmd.py`) | read |

Spot-checks against the repo (5 of 5 allowed; `git fetch origin`, then `git show origin/main:<path>`, origin/main = 2cbb3b71f):

| # | Claim | Result | Ev |
|---|---|---|---|
| 1 | `pi/lib/startup.ts:32` is `if (ctx.mode !== "tui") return;` | confirmed at line 32, first line of `start()` | measured (git show) |
| 2 | `pi/durable-host/run.ts:231` builds `hostExtension` with `visible: []` | confirmed (the `base = hostExtension(...)` line) | measured (git show) |
| 3 | `run.ts:469` `configure` throws `refused` when `pending > 0` | confirmed (`configure` at 468, guard at 469) | measured (git show) |
| 4 | `protocol.ts:114` `KINDS` has no bash, tool-row or footer kind | confirmed: hello, attach, submit, compact, configure, list_models, fork, history, abort, abort_submission, ping, detach, reload, ui_response | measured (git show) |
| 5 | `run.ts:495-500` `fork` validates `at`, then calls `cpd session fork <session>` without it | confirmed (`at` checked at 496, call at 499) | measured (git show) |

- `docs/research/pi-durable-deep-dive.md` does not exist on origin/main (`git ls-tree origin/main docs/research/` lists durable-leads, dead-durable-leads, pi-durable-resume, durable-first-turn-smoke, pi-durable, ratatui-chat-durable-ui only), so no pointer is given; sibling Plan pi-durable-how owns Chord, storage and checkpoints (`measured`).

### 2.2 Corpus, classification rule, snapshot

- Interactive = `sessions` rows with `role IN ('manager','lead')`; worker rows are counted but excluded; `file` is the JSONL path (`read` `prompts.py:59`, `features.py:27`).
- Snapshot: `prompts.py` pins a cutoff `CUTOFF = '2026-11-03T12:00:00.000Z'` (07:00 EST) and ignores db rows, files named after `2026-11-03T12-00-00` and entries stamped after it (`read` `prompts.py:9-10,66,102`); `features.py` has no cutoff (`measured`: `grep -n CUTOFF` finds it only in `prompts.py`).
- Both open the db read-only: `sqlite3.connect('file:%s?mode=ro' % DB, uri=True)` (`read` `prompts.py:56`, `features.py:24`). Reruns are deterministic: two back-to-back runs gave `cmp` exit 0 (`measured`, use-prompts head; use-features "Determinism check").
- Typed-vs-injected rule, first match wins, applied to `role: user` messages only (`read` `prompts.py:23-35`):

```python
RULES = [
 ('file_brief',      lambda t, first: t.lstrip().startswith('<file name=')),
 ('first_msg_brief', lambda t, first: bool(re.match(r'\s*You are the (Manager|Lead) of ', t))),
 ('message_from',    lambda t, first: t.startswith('Message from ')),
 ('cpd_context',     lambda t, first: bool(re.match(r'Context \S+: \d+ new records', t)) or '<cpd-context' in t[:200]),
 ('plan_review',     lambda t, first: bool(re.match(r'Plan review \S+ was (APPROVED|REJECTED|CHANGES)', t)) or t.startswith('Plan review ')),
 ('cpd_adapter',     lambda t, first: t.startswith('cpd adapter:')),
 ('cpd_event',       lambda t, first: t.startswith('From cpd') or t.startswith('cpd reminder') or t.startswith('cpd:') or t.startswith('Skill cpd-')),
]
def classify(t, first):
    for n, f in RULES:
        if f(t, first): return n
    return 'typed'
```

- Result of the rule: 976 user-role messages = 227 `file_brief` + 749 `typed`; the other six rules matched 0 (`measured`, use-prompts "Counts for the 976"). Spot-check of 20 per class (seed 20261103) agreed 20/20 for typed (`measured`, `/tmp/pi-chat-features/spotcheck.txt`).
- Limits (all labelled): slash commands and `!bash` are not persisted by pi's TUI, so 0 is an undercount (`inferred`, use-prompts "Limits"); images and `@file` may leave no entry (`inferred`); entries are counted across the whole file, not the active branch (`read`); 3 interactive rows are unmeasured, 2 with `file` NULL and 1 file missing (`measured`); `typed` cannot be proven human from the file (`inferred`).
- Every measured session ran on pi's own TUI: use-prompts finds "no durable runner row" among the interactive rows and use-features finds 269 rows with runner `pi` only (`measured`). Usage therefore says nothing about features that exist only on the durable-host path (`inferred`).

### 2.3 Why the interactive session counts differ (242 vs 269)

| Quantity | use-prompts (cut at 2026-11-03T12:00Z) | use-features (uncut) | Ev |
|---|---|---|---|
| interactive db rows | 245 (lead 230, manager 15) | 269 (lead 252, manager 17) | measured |
| interactive resolved / loaded | 242 (lead 229, manager 13) | 266 (lead 251, manager 15) | measured |
| `file` NULL / file missing | 2 / 1 | 2 / 1 | measured |
| worker rows | 2,839 | 3,026 | measured |
| JSONL bytes read | 520,678,460 | 543,901,014 | measured |
| message entries (all roles) | 71,422 | 74,429 | measured |
| assistant turns | 34,678 | 36,127 | measured |
| user-role messages | 976 | 1,035 | measured |
| `cpd-context` custom_message | 1,525 | 1,597 | measured |
| `cpd-plan-decision` | 225 | 240 | measured |
| `cpd-plan-summary` | 238 | 252 | measured |
| `cpd-skills` | 71 | 105 | measured |
| all `custom_message` | 4,657 | 4,960 | measured |

- Reason (`measured`): use-prompts bounds everything at the 2026-11-03T12:00Z cutoff, use-features reads the live corpus with no bound. The difference is 269 - 245 = 24 db rows (lead 22, manager 2) and 266 - 242 = 24 loaded sessions, created after the cutoff, plus entries appended after the cutoff to sessions that existed at it. The three unmeasured rows are the same in both.
- Third count (`read`): use-prompts quotes `stopReason: error` 472 "across all 265 sessions in an uncut earlier read"; neither 242 nor 266, and not in the script output, so it is not used here.
- Rule of use (`inferred`): prompt, steer and abort numbers come from use-prompts (242, cut); tool, dialog, compaction, model, thinking and context numbers come from use-features (266, uncut). Section 6 labels each table with its source.

### 2.4 Conflicts between artefacts (none silently resolved)

| # | Conflict | inv / use A | inv / use B | Status | Ev |
|---|---|---|---|---|---|
| 1 | session counts | use-prompts 242 | use-features 266/269 | explained above by the cutoff | measured |
| 2 | rpc behaviour of `setWorkingMessage`, `setWidget`, `custom` | inv-pi: no-ops in rpc (`rpc-ext-ui` Limitations, `RPC:62-184`) | inv-cpd: "no-op or an extension_ui_request, unverified"; no installed pi was read | unresolved; this report follows inv-pi for the class and marks it | read |
| 3 | why `cpd_secret_approve` and `cpd_host_approve` have 0 calls | use-features: "may exist only in durable/worker runners" | inv-cpd: in the TUI a Manager never gets either tool (registered inside the `planLead` branch, `pi/lib/startup.ts:157,190-191`), a Plan Lead gets only secret_approve | unresolved; both explanations fit 0 | read |
| 4 | who changes models | inv-cpd: "no interactive tier or model selector", changes are Event-driven | inv-pi: `/model` selector exists; use-prompts could not count `/model` (not persisted); use-features: 652 `model_change`, 342 of 386 transitions name the same model | unresolved: User `/model` use is unmeasurable (`inferred`) | measured |
| 5 | secret_approve role | inv-cpd: tool text says "Lead and Manager" | inv-cpd: registered for Plan Leads only (`startup.ts:157,190`) | a contradiction inside the source, not between artefacts; host registers for both roles (gap-durable 1.10) | read |
| 6 | `host_state.agent` | gap-durable 1.4: `agent_changed` is in the snapshot | gap-durable 1.4: `host_state.agent` is empty until the first `configure` | both hold; the frame is empty, the stream is not | read |

## 3. Inventory

Legend (`read`, inv-pi Abbreviations): all pi paths are under `/Users/Clayton.Duarte/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/`; `CA` = `pi-coding-agent`; `DOC` = `CA/docs`; `IM` = `CA/dist/modes/interactive/interactive-mode.js`; `COMP` = `CA/dist/modes/interactive/components`; `RPC` = `CA/dist/modes/rpc/rpc-mode.js`; `RT` = `CA/dist/modes/rpc/rpc-types.d.ts`; `SC` = `CA/dist/core/slash-commands.js`; `AS` = `CA/dist/core/agent-session.js`; `EXT` = `CA/dist/core/extensions/types.d.ts`; `ED` = `pi-tui/dist/components/editor.js`; `AC` = `pi-tui/dist/autocomplete.js`. cpd rows cite `pi/...` of cpd origin/main. Tags: `tui-patch` = prototype or instance wrapping of pi-tui/pi internals; `ui-api` = public extension API; rpc column = exposed by `pi --mode rpc`.

- RPC exposes the agent engine (31 commands, `RPC:298-540`) and an extension-UI subprotocol, but no builtin slash command text, no selector, no composer, no TUI rendering (`read`, inv-pi short answer; `docs/rpc-commands.md` get_commands Note).
- Row counts: 87 baseline + 44 cpd = 131; IDs are the join key of sections 4 and 5 (`read`).

### 3.1 pi-agent baseline (pi-coding-agent 1.3.0 + pi-tui)


### 3.1.1 Composer

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| C01 | @file mention | Type @, fuzzy file picker (fd); inserts the path as plain text | `AC:90-124; IM:545-559` | no | read |
| C02 | Path completion (Tab) | Tab completes a filesystem path | `AC:315` | no | read |
| C03 | !cmd (bash into context) | Runs locally, bordered block, output enters model context | `IM:2689-2705; RPC:439-465` | yes (bash, abort_bash) | read |
| C04 | !!cmd (bash excluded from context) | Same, output not sent to the model | `IM:2689-2693; RPC:439-457` | yes (excludeFromContext) | read |
| C05 | Bash-already-running guard | Warns "Esc to cancel first", keeps the text | `IM:2695-2699` | partly | inferred |
| C06 | Builtin slash dispatch (typed /x) | Menu of 23 builtin commands; Enter runs one | `SC:2-26; IM:2542-2688` | no | read |
| C07 | Extension slash commands | pi.registerCommand names in the same menu | `IM:497-505; AS:1611-1615` | yes (get_commands + prompt) | read |
| C08 | Prompt-template commands | Template names expand as /name | `IM:483-489; AS:1631` | yes | read |
| C09 | Skill commands (/skill:name) | Skills as commands when enableSkillCommands is on | `IM:507-518; RPC:559-566` | yes | read |
| C10 | Image paste / attach | Clipboard image becomes a temp-file path; images[] blocks accepted by the engine | `IM:2479-2540; RT:16-27` | partly (images[] yes) | read |
| C11 | Multi-line editing keys | Shift+Enter, kill-ring, undo, word moves | `ED:233-262,1108-1138` | no | read |
| C12 | Large-paste collapse | >10 lines or >1000 chars become [paste #N] marker | `ED:1050-1107` | no | read |
| C13 | Input history (Up/Down) | Browse previous prompts, draft restored | `ED:308-366` | no | read |
| C14 | Autocomplete menu | Slash names, args, templates, skills, @files; providers pluggable | `IM:465-559; RPC:155-157` | no (names only via get_commands) | read |
| C15 | Keybindings config | keybindings.json, every action rebindable | `DOC/keybindings.md; IM:5704` | no | read |
| C16 | External editor (Ctrl+G) | Opens $EDITOR on the composer text | `IM:3771` | no | read |
| C17 | Double-Esc selector | Esc twice on empty editor opens tree or fork selector | `IM:2418-2450` | no | read |
| C18 | Suspend / exit keys | Ctrl+Z, Ctrl+D, Ctrl+C x2 | `IM:3465-3483,3623` | no | read |
| C19 | Extension input transform | pi.on("input") rewrites or swallows typed text | `AS:1561-1572; EXT:1227` | yes (source "rpc") | read |
| C20 | Extension shortcuts | pi.registerShortcut binds keys | `EXT:1233; IM:1714` | no | read |

### 3.1.2 Slash commands (builtin)

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| S01 | /settings (+ settings selector) | Large settings list | `IM:2548; COMP/settings-selector.js:37-627` | partly (4 setters) | read |
| S02 | /model (+ selector, Ctrl+P cycle) | Model selector or direct select | `IM:2558,4454; RPC:365-387` | yes | read |
| S03 | /tree (+ tree selector, navigation) | Navigate session tree, optional branch summary | `IM:2626,4641-4764` | partly (get_tree read only) | read |
| S04 | /thinking (+ selector, Shift+Tab cycle) | Set thinking level | `IM:2564,3695-3735` | yes | read |
| S05 | /scoped-models | Pick models for cycling | `IM:2553,4480` | no | read |
| S06 | /export | HTML or JSONL export | `IM:2570,5445` | partly (export_html only) | read |
| S07 | /import | Import and resume a JSONL session | `IM:2575,5488` | partly (switch_session, unverified) | inferred |
| S08 | /share | Upload session as artifact or gist | `IM:2580,5530` | no | read |
| S09 | /bug | Prepare a private bug report | `IM:2585,5540` | no | read |
| S10 | /copy | Copy last assistant message | `IM:2591,5551` | partly (get_last_assistant_text) | read |
| S11 | /name (rename) | Set or show session name | `IM:2596,5577; RPC:523-533` | yes | read |
| S12 | /session (stats) | File, id, counts, tokens, cost | `IM:2601,5601` | yes (get_session_stats) | read |
| S13 | /changelog | Show changelog | `IM:2606,5675` | no | read |
| S14 | /hotkeys | List shortcuts | `IM:2611,5704` | no | read |
| S15 | /fork (+ fork selector) | New session from an earlier user message | `IM:2616,4593-4621` | yes (get_fork_messages + fork) | read |
| S16 | /clone | Duplicate session at current position | `IM:2621,4622` | yes (clone) | read |
| S17 | /trust (+ trust selector) | Save project trust decision | `IM:2631,4432` | no | read |
| S18 | /login /logout (+ OAuth dialogs) | Provider auth dialogs | `IM:2636,4825-5368` | no | read |
| S19 | /new | Start a new session | `IM:2647,5815; RPC:334-344` | yes (new_session) | read |
| S20 | /compact [instructions] | Manual compaction | `IM:2652,5941; RPC:417-420` | yes | read |
| S21 | /resume (+ session selector) | Pick and resume a session | `IM:2678,4765` | partly (switch_session, no list) | read |
| S22 | /reload | Reload extensions, skills, themes | `IM:2658,5369` | no (ctx.reload only) | read |
| S23 | /quit | Quit pi | `IM:2683` | no | read |
| S24 | /llama, /debug, easter eggs | Bundled-extension and hidden commands | `IM:2663,5069` | no | read |

### 3.1.3 Mid-run controls

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| M01 | Steer (Enter while running) | Queued after the current tool round | `IM:2722-2731; RPC:298-323` | yes | read |
| M02 | Follow-up (Alt+Enter) | Queued until the agent finishes | `IM:3655-3685; RPC:323-326` | yes | read |
| M03 | Queue modes (all / one-at-a-time) | steering-mode, follow-up-mode settings | `COMP/settings-selector.js:277-284; RPC:406-415` | yes | read |
| M04 | Queued message display | Dim "Steering:/Follow-up:" lines under the transcript | `IM:3875-3891` | partly (queue_update event) | read |
| M05 | Dequeue to editor (Alt+Up) | Clear queues, paste texts into the composer | `IM:3686-3694,3893-3911` | partly (clear_queue) | read |
| M06 | Escape abort | Abort run, restore queue; abort bash, retry, compaction | `IM:2418-2437; RPC:327-331,432-460` | yes (abort, abort_bash, abort_retry) | read |
| M07 | Ctrl+C clear / quit | Clear editor, then quit | `IM:3465-3473` | no | read |
| M08 | Queue during compaction | Messages held until compaction ends | `IM:2708-2720` | no (TUI-side hold) | read |
| M09 | Working indicator | Spinner + "Working..." while a turn runs | `IM:1776-1838,2760-2773` | partly (events) | read |
| M10 | Auto-retry display and toggle | "Retrying (n/m)" with Esc to cancel | `IM:3036-3060; RPC:428-437` | yes | read |
| M11 | Terminal progress (OSC) | Terminal progress bar while a turn runs | `IM:2760-2766` | no | read |

### 3.1.4 Rendering

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| R01 | Streaming assistant text | Text grows delta by delta | `IM:2827-2912; json.md` | yes (data) | read |
| R02 | Markdown (+ Mermaid, LaTeX) | Themed markdown rendering | `COMP/assistant-message.js:1-30` | no (raw text) | read |
| R03 | Tool-call rendering | Coloured box per call, renderCall/renderResult, duration | `IM:2851-2955; COMP/tool-execution.js:24-243` | yes (data) | read |
| R04 | Tool output collapse / expand (Ctrl+O) | Toggle all tool output | `IM:3736-3756` | no | read |
| R05 | Diffs | Coloured diff on edit calls | `COMP/diff.js; IM:2890-2893` | no (data) | inferred |
| R06 | Thinking blocks (+ hide) | Dim italic, Ctrl+T hides | `IM:3765-3770; COMP/assistant-message.js:12-21` | partly (data) | read |
| R07 | Thinking level / model display | Editor border colour, footer model | `IM:3695-3735,2823-2826` | yes (data) | read |
| R08 | Compaction summary display | Collapsed box with token count | `IM:2971-3035; COMP/compaction-summary-message.js` | yes (data) | read |
| R09 | Branch summary | Box after tree navigation | `COMP/branch-summary-message.js` | partly | read |
| R10 | Bash execution blocks | Bordered live output, exit code, truncation | `COMP/bash-execution.js:5-39` | yes (data) | read |
| R11 | Images in output | Inline images (Kitty/iTerm2) | `COMP/tool-execution.js:265-296` | no (data) | read |
| R12 | Footer / status line | cwd, branch, model, context %, tokens, cost | `COMP/footer.js:31-120` | partly | read |
| R13 | Widgets | Text blocks above/below the editor | `IM:1853-1957; RPC:84-97` | partly (string[] only) | read |
| R14 | Header / loaded resources | Startup header | `IM:1327-1548` | no | read |
| R15 | Themes | dark/light/custom/system, hot reload | `DOC/themes.md` | no | read |
| R16 | Error / status / notice lines | showStatus, showError, notify | `IM:3115,3796-3830; RPC:48-57` | mostly no | read |
| R17 | Custom messages / entries | Extension-defined entries rendered by renderers | `EXT:1250-1254; IM:2778-2817` | partly (data) | read |
| R18 | Fullscreen viewport | Scroll, scrollbar, selection copy, prompt jump, search | `DOC/keybindings.md "Fullscreen"; IM:605` | no | read |
| R19 | Terminal title | Window or tab title updates | `IM:814; RPC:104-112` | partly (setTitle) | read |

### 3.1.5 Dialogs and extension UI

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| D01 | ctx.ui.select | List picker with optional timeout | `IM:2141; RPC:44-46` | yes | read |
| D02 | ctx.ui.confirm | Yes/no | `IM:2153` | yes | read |
| D03 | ctx.ui.input | Single-line prompt | `IM:2167` | yes | read |
| D04 | ctx.ui.editor | Multi-line editor dialog with prefill | `IM:2210; RPC:135-154` | yes | read |
| D05 | ctx.ui.notify | Transient message | `IM:2312` | yes | read |
| D06 | setStatus / setWidget / setTitle | Extension chrome | `RPC:62-112` | yes (string-only widgets) | read |
| D07 | setEditorText / pasteToEditor | Prefill the composer | `RPC:117-134` | yes (set only) | read |
| D08 | ctx.ui.custom (overlay components) | Full custom component | `IM:2324; RPC:113-116` | no | read |
| D09 | TUI-internal UI APIs | onTerminalInput, setEditorComponent, setFooter/Header, setTheme | `RPC:58-61,98-103,158-177` | no | read |
| D10 | Dialog timeout | Auto-resolve with a default | `RPC:7-40` | yes | read |

### 3.1.6 Session management

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| N01 | Auto-compaction | Threshold and overflow compaction, toggle | `COMP/settings-selector.js:270; RPC:421-427` | yes | read |
| N02 | Message history | Transcript rebuilt on resume | `IM:1688; RPC:503-514` | yes (get_messages) | read |
| N03 | Session lifecycle hooks | session_before_switch/fork/compact/tree | `EXT:1189-1199` | yes | read |


### 3.2 cpd additions (the four extensions of `pi/package.json` `pi.extensions`)

- Provenance (`read`, inv-cpd 0): `compact-tools/` is a copy of upstream pi-compact-tools 0.16.6 and `ask-user-question/` of `@juicesharp/rpiv-ask-user-question` v2.13.8 (their `NOTICE` files); a web UI rebuilds both either way.
- Gate (`read`, inv-cpd 0): `pi/lib/startup.ts:32` returns before the Run, follower, tools or commands exist, so in rpc the guards (`adapter.ts:75,79,87` need `host.run`), `cpd_plan_review`, `cpd_secret_approve`, `cpd_host_approve` and `/plan-review` do not exist.

### 3.2.1 Composer and commands

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| P01 | /reload auto-trigger after install | Adapter poll sends the reload command when idle | `pi/lib/reload-command.ts:7-29; lib/startup.ts:204-216` | ui-api; rpc inert | read |
| P02 | /plan-review (reopen pending review) | Reopens the Approve/Change dialog | `pi/lib/plan-review-command.ts:12-35` | ui-api; rpc inert | read |
| P03 | /quit marker | Writes CPD_QUIT_MARKER before pi quits | `pi/lib/quit.ts:16-35` | tui-patch; TUI only | read |
| P04 | Compaction reserve ratio | Large windows compact near the limit | `pi/lib/compaction-ratio.ts:77-146` | tui-patch (core prototypes) | read |
| P05 | Tier-driven model follow (retier) | Silent model switch at idle on session-retiered | `pi/lib/retier.ts:8-66` | ui-api; rpc inert | read |
| P06 | Mentions painting | Session/plan/context ids painted magenta italic | `pi/lib/mentions.ts:48-63` | tui-patch; TUI only | read |
| P07 | Typed-message reply flag | A typed message during a run marks it a reply | `pi/lib/agent-state.ts; lib/reply.ts` | ui-api; rpc inert | read |
| P08 | /compact-tools style picker | Style and scope pickers | `pi/compact-tools/compact-tools-command.ts:86-192` | ui-api; TUI only | read |
| P09 | /silent and ctrl+' | Show only prompts and final answers | `pi/compact-tools/compact-tools-silent.ts:406-500` | tui-patch; TUI only | read |
| P10 | Jump hint and question-header restyle | Grey hint, grey pill header | `pi/lib/hint.ts:44-72` | tui-patch | read |
| P11 | Multi-question tab bar restyle | Pill tabs joined by lines | `pi/lib/hint.ts:25-58` | tui-patch (class-name match) | read |
| P12 | Chat pad | One blank cell around the layout | `pi/lib/pad.ts:26-83` | tui-patch | read |
| P13 | Editor box | Rounded box, state-coloured border | `pi/compact-tools/compact-tools-editor.ts:149-235` | tui-patch | read |
| P14 | Input cursor | Blinking cycling cursor when focused | `pi/compact-tools/compact-tools-cursor.ts:46-128` | tui-patch | read |
| P15 | Viewport keeper | Keeps text anchored when rows above change | `pi/compact-tools/compact-tools-viewport.ts:73-234` | tui-patch | read |

### 3.2.2 Rendering and notices

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| P16 | Compact tool rows (fold) | Head, rail, expand/collapse, diff counts, time | `pi/compact-tools/compact-tools.ts:848-855; compact-tools-custom.ts:55-107` | ui-api or tui-patch | read |
| P17 | Grouped rows (claude/codex styles) | "Searched for N patterns, read M files" | `pi/compact-tools/compact-tools-grouping.ts:36-372` | tui-patch | read |
| P18 | Collapsible thinking | Folded "Thinking..." by default | `pi/compact-tools/compact-tools-thinking.ts:253-385` | tui-patch | read |
| P19 | Compaction fold and "Compacting..." cue | Fold with summary on rails; running widget | `pi/compact-tools/compact-tools-compaction.ts:2-132` | tui-patch | read |
| P20 | Working label, loader, progress words | Moon loader, pill label, progress in silent mode | `pi/compact-tools/compact-tools-progress.ts:46-154` | ui-api + tui-patch | read |
| P21 | Dango theme (16 indexed colours) | Fixed palette by terminal index | `pi/themes/dango.json; compact-tools-indices.ts:1-105` | ui-api (data file) | read |
| P22 | Dango italics | SGR 3 on four tokens | `pi/compact-tools/compact-tools-italic.ts:1-17` | tui-patch | read |
| P23 | Balloons (user and agent bubbles) | Rounded boxes, agent tail with local HH:MM | `pi/compact-tools/compact-tools-balloon.ts:1-49` | tui-patch | read |
| P24 | Pills | Inverted-text labels, 8 meanings, 3 clickable remaps | `pi/lib/pill.ts:3-53; pill-spec.json` | ui-api (string) | read |
| P25 | Time display and zone line | HH:MM in folds, zone told to the model each turn | `pi/lib/time.ts:18-56` | ui-api | read |
| P26 | cpd message folds | Fold for cpd, cpd-context, plan-summary, plan-decision, skills | `pi/lib/render.ts:23,150-238` | ui-api | read |
| P27 | cpd-context notice | One fold per turn start for Lead/Manager | `pi/lib/updates.ts:22-31; lib/turns.ts:69-73` | ui-api | read |
| P28 | Skills-changed notice | Fold after the role skill changed | `pi/lib/skills.ts:71-108` | ui-api | read |

### 3.2.3 Dialogs

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| P29 | ask_user_question schema, validation, G2 limit | 1-4 questions, 2-4 options, 1500-char limit | `pi/ask-user-question/tool/types.ts:83-91; lib/askguard.ts:4-32` | ui-api | read |
| P30 | ask_user_question answer kinds | option / multi / custom / cancel | `pi/ask-user-question/tool/types.ts:109-162` | ui-api | read |
| P31 | ask_user_question notes, global note, partial submit | Per-question notes, Submit tab warning | `pi/ask-user-question/state/state-reducer.ts:280-304` | ui-api | read |
| P32 | ask_user_question tabs, preview pane, multi-select rows | Tab per question, side preview, Space toggles | `pi/ask-user-question/view/; state/key-router.ts` | ui-api | read |
| P33 | ask_user_question collapse, displaced, hang-guard, bell | Overlay collapse key, 1 s mount check | `pi/ask-user-question/hang-guard.ts:20-85` | tui-patch | read |
| P34 | ask_user_question 2 h stuck bound and no_ui | dialog_stuck after 2 h; no_ui without a UI | `pi/ask-user-question/ask-user-question.ts:294-300,402` | ui-api | read |
| P35 | ask_user_question G4 label visibility | Refuses labels stated nowhere | `pi/lib/askguard.ts:55-103` | ui-api | read |
| P36 | cpd_plan_review dialog | Approve/Change select, Change editor, Esc "still waiting" | `pi/lib/review.ts:35-198; lib/review-chat.ts:42-66` | ui-api | read |
| P37 | cpd_secret_approve dialog | Allow once / Deny select | `pi/lib/secret.ts:12-57` | ui-api | read |
| P38 | cpd_host_approve dialog | Allow once / Deny select, Manager only | `pi/lib/hostapprove.ts:28-59` | ui-api | read |
| P39 | MCP attach and mcp login | pi MCP extension plus attach Events | `pi/lib/mcp.ts:10-21` | ui-api | read |
| P40 | Dialog-state reporting | Session reports blocked while a dialog is open | `pi/lib/state.ts:84-107` | ui-api | read |

### 3.2.4 Guards and Event delivery

| ID | Feature | What the User does or sees | Source | rpc / tag | Ev |
|---|---|---|---|---|---|
| P41 | Loop-guard visibility | Trip holds state blocked and queued deliveries | `pi/lib/loop-guard.ts:16-125` | ui-api | read |
| P42 | Agent-side guards (secret, hostapprove, main, noblock, G2) | Refusals shown as tool results | `pi/lib/adapter.ts:69-107` | ui-api | read |
| P43 | Hidden turn plumbing | reminder, first-message, policy and zone sections, state reporter | `pi/lib/reminder.ts; lib/first-message.ts; lib/agent-state.ts` | ui-api | read |
| P44 | Event delivery (follower, cursor, steer vs follow-up) | Events become turns | `pi/lib/follower.ts; lib/handle.ts:97-169; lib/delivery.ts:10-21` | ui-api (public API) | read |


## 4. Gap matrix: pi-agent vs raw pi-durable vs pi/durable-host

- Columns (`read`, gap-durable 1): pi-agent = exists as an interactive feature of pi 1.3.0 (`yes`/`partly`/`no`, `cpd` = a cpd addition); raw pi-durable = what `@earendil-works/pi-durable` 1.3.0 gives (`yes`/`partly`/`none`); pi/durable-host marks `ok`, `stream-only` (data rides the untyped `events`/`snapshot` frames, `protocol.ts:85-86`, the client renders), **degraded**, **missing**, `n/a` (a client-only concern with nothing for the host to carry).
- Pin (`read`, gap-durable 0): cpd pins pi-durable 1.3.0 in the root `package.json:32`; latest on the registry is 1.5.0, which removes `watchEvents` and `viewState` that the host uses (`pi/durable-worker/harness.ts:5,50,77`; 1.5.0 `CHANGELOG.md:49,56`; npm query `measured` 2026-11-03 EST).
- Raw pi-durable `Conversation` has exactly `agent, configure, submit, reset, compact, context, entries, fork, abort, waitForIdle, viewState, watch` (`read` `types.d.ts:589-636`); keyword searches for theme, footer, autocomplete, keybind, slash, oauth, navigate, label, rename, trust return 0 hits (`measured`, `/tmp/pi-chat-features/pd-search.out`).
- The host is a headless socket server and the repo has no cpd client of it (`read`, gap-durable 0.1), so a gap is either a missing wire message or work every client must do.

### 4.1 Matrix (one row per inventory row)

| ID | Feature | pi-agent | raw pi-durable | pi/durable-host | Mark | Host evidence | Ev |
|---|---|---|---|---|---|---|---|
| C01 | @file mention | yes | none | missing | **missing** | text only via submit (protocol.ts:42) | read (gap-durable 1.1) |
| C02 | Path completion (Tab) | yes | none | missing | **missing** | same as C01 | read (gap-durable 1.1) |
| C03 | !cmd (bash into context) | yes | partly | missing | **missing** | no bash kind in KINDS (protocol.ts:114, spot-checked) | read (gap-durable 1.1) |
| C04 | !!cmd (bash excluded from context) | yes | partly | missing | **missing** | same as C03 | read (gap-durable 1.1) |
| C05 | Bash-already-running guard | partly | none | n/a | n/a | no user bash on the host | inferred (gap-durable 1.1) |
| C06 | Builtin slash dispatch (typed /x) | yes | none | missing | **missing** | typed /x is raw submit.text (run.ts:454-459) | read (gap-durable 1.1) |
| C07 | Extension slash commands | yes | none | missing | **missing** | host registers tools only (tools.ts:30-135) | read (gap-durable 1.1) |
| C08 | Prompt-template commands | yes | none | missing | **missing** | no template code in host | read (gap-durable 1.1) |
| C09 | Skill commands (/skill:name) | yes | none | degraded | **degraded** | role skill is a prompt section, no /skill: (skills.ts:21-25) | read (gap-durable 1.1) |
| C10 | Image paste / attach | partly | yes | degraded | **degraded** | submit.images max 4, 786 432 base64 bytes (protocol.ts:19-21); read tool has no image processor (harness.ts:96) | read (gap-durable 1.1) |
| C11 | Multi-line editing keys | yes | none | n/a | n/a | client concern | read (gap-durable 1.1) |
| C12 | Large-paste collapse | yes | none | n/a | n/a | 1 MiB line limit only (protocol.ts:3) | read (gap-durable 1.1) |
| C13 | Input history (Up/Down) | yes | partly | degraded | **degraded** | history pages entries (run.ts:508-529) | read (gap-durable 1.1) |
| C14 | Autocomplete menu | yes | none | missing | **missing** | no command list; list_models exists (protocol.ts:45) | read (gap-durable 1.1) |
| C15 | Keybindings config | yes | none | n/a | n/a | - | read (gap-durable 1.1) |
| C16 | External editor (Ctrl+G) | yes | none | n/a | n/a | - | read (gap-durable 1.1) |
| C17 | Double-Esc selector | yes | none | n/a | n/a | - | read (gap-durable 1.1) |
| C18 | Suspend / exit keys | yes | none | n/a | n/a | detach closes one client (protocol.ts:51) | read (gap-durable 1.1) |
| C19 | Extension input transform | yes | none | n/a | n/a | not used by the host | read (gap-durable 1.1) |
| C20 | Extension shortcuts | yes | none | n/a | n/a | - | read (gap-durable 1.1) |
| S01 | /settings (+ settings selector) | yes | yes | missing | **missing** | Harness.open gets no settings (harness.ts:99-103) | read (gap-durable 1.2) |
| S02 | /model (+ selector, Ctrl+P cycle) | yes | yes | degraded | **degraded** | configure refused while a run is pending (run.ts:469, spot-checked); no cycle | read (gap-durable 1.2) |
| S03 | /tree (+ tree selector, navigation) | yes | none | missing | **missing** | attach accepts only conversation "root" (server.ts:118-122) | inferred (gap-durable 1.2) |
| S04 | /thinking (+ selector, Shift+Tab cycle) | yes | yes | degraded | **degraded** | same pending guard (run.ts:469); levels via list_models | read (gap-durable 1.2) |
| S05 | /scoped-models | yes | none | missing | **missing** | - | read (gap-durable 1.2) |
| S06 | /export | yes | none | missing | **missing** | history gives raw entries (run.ts:508) | read (gap-durable 1.2) |
| S07 | /import | yes | none | missing | **missing** | one fixed store per Session (config.ts:25-27) | inferred (gap-durable 1.2) |
| S08 | /share | yes | none | missing | **missing** | - | read (gap-durable 1.2) |
| S09 | /bug | yes | none | missing | **missing** | - | read (gap-durable 1.2) |
| S10 | /copy | yes | none | n/a | n/a | client reads last message_end | read (gap-durable 1.2) |
| S11 | /name (rename) | yes | none | degraded | **degraded** | host_state.name read once (run.ts:545); session-renamed has no handler | read (gap-durable 1.2) |
| S12 | /session (stats) | yes | partly | degraded | **degraded** | usage_changed rides the stream; no cost or context-% computed | read (gap-durable 1.2) |
| S13 | /changelog | yes | none | missing | **missing** | - | read (gap-durable 1.2) |
| S14 | /hotkeys | yes | none | n/a | n/a | - | read (gap-durable 1.2) |
| S15 | /fork (+ fork selector) | yes | yes | degraded | **degraded** | fork validates at then ignores it (run.ts:495-500, spot-checked); ownership always rejected | read (gap-durable 1.2) |
| S16 | /clone | yes | partly | missing | **missing** | use fork at head (same at defect) | read (gap-durable 1.2) |
| S17 | /trust (+ trust selector) | yes | none | missing | **missing** | host loads context files with no trust check (main.ts:41) | read (gap-durable 1.2) |
| S18 | /login /logout (+ OAuth dialogs) | yes | none | missing | **missing** | mcp_login purpose declared, never emitted (protocol.ts:62) | read (gap-durable 1.2) |
| S19 | /new | yes | yes (reset) | missing | **missing** | no reset kind (protocol.ts:114); drops Lead context, decide first | read (gap-durable 1.2) |
| S20 | /compact [instructions] | yes | yes | degraded | **degraded** | compact{instructions} gives task_id only; outcome not reported | read (gap-durable 1.2) |
| S21 | /resume (+ session selector) | yes | yes | ok | ok | attach{cursor} resumes the stream (server.ts:117-131); sessions listed by cpd session list | read (gap-durable 1.2) |
| S22 | /reload | yes | yes | ok | ok | reload message (protocol.ts:52, reload.ts:26-39) | read (gap-durable 1.2) |
| S23 | /quit | yes | none | n/a | n/a | detach only; engine stops hosts (main.ts:48-51) | read (gap-durable 1.2) |
| S24 | /llama, /debug, easter eggs | partly | none | n/a | n/a | - | read (gap-durable 1.2) |
| M01 | Steer (Enter while running) | yes | yes | ok | ok | submit.when_busy steer (protocol.ts:42; run.ts:452-465) | read (gap-durable 1.3) |
| M02 | Follow-up (Alt+Enter) | yes | yes | ok | ok | default when_busy followUp (protocol.ts:184) | read (gap-durable 1.3) |
| M03 | Queue modes (all / one-at-a-time) | yes | yes | missing | **missing** | no settings passed (harness.ts:99-103) | read (gap-durable 1.3) |
| M04 | Queued message display | partly | yes (inbox_update) | degraded | **degraded** | loop-guard held inputs return id -1 and are not in the inbox (run.ts:377-380) | read (gap-durable 1.3) |
| M05 | Dequeue to editor (Alt+Up) | partly | partly | degraded | **degraded** | abort_submission per item only (protocol.ts:49) | read (gap-durable 1.3) |
| M06 | Escape abort | yes | yes | ok | ok | abort{background} resolves held dialogs as aborted (run.ts:530-538) | read (gap-durable 1.3) |
| M07 | Ctrl+C clear / quit | yes | none | n/a | n/a | - | read (gap-durable 1.3) |
| M08 | Queue during compaction | yes | yes | ok | ok | pi-durable keeps working during compaction natively | read (gap-durable 1.3) |
| M09 | Working indicator | partly | yes | degraded | **degraded** | host_state.state idle/working/waiting/blocked (protocol.ts:55-60); waiting never set; status kind has no emitter | read (gap-durable 1.3) |
| M10 | Auto-retry display and toggle | yes | yes | degraded | **degraded** | auto_retry_* events in stream; no toggle or abort_retry wire | read (gap-durable 1.3) |
| M11 | Terminal progress (OSC) | yes | none | n/a | n/a | - | read (gap-durable 1.3) |
| R01 | Streaming assistant text | yes | yes | stream-only | stream-only | events frames carry unknown (protocol.ts:85-86) | read (gap-durable 1.4) |
| R02 | Markdown (+ Mermaid, LaTeX) | yes | none | n/a | n/a | client renders | read (gap-durable 1.4) |
| R03 | Tool-call rendering | yes | yes | stream-only | stream-only | no wire kind for tool rows | read (gap-durable 1.4) |
| R04 | Tool output collapse / expand (Ctrl+O) | yes | none | n/a | n/a | - | read (gap-durable 1.4) |
| R05 | Diffs | yes | yes | stream-only | stream-only | diff in tool result details (inferred, not opened) | inferred (gap-durable 1.4) |
| R06 | Thinking blocks (+ hide) | yes | yes | stream-only | stream-only | thinking_* events in stream | read (gap-durable 1.4) |
| R07 | Thinking level / model display | yes | yes | degraded | **degraded** | host_state.agent empty until the first configure (run.ts:343,471,545) | read (gap-durable 1.4) |
| R08 | Compaction summary display | yes | yes | stream-only | stream-only | compaction_start/end in stream | read (gap-durable 1.4) |
| R09 | Branch summary | yes | none | missing | **missing** | needs S03 | inferred (gap-durable 1.4) |
| R10 | Bash execution blocks | yes | yes | stream-only | stream-only | bash tool streams output | read (gap-durable 1.4) |
| R11 | Images in output | yes | yes | stream-only | stream-only | base64 inside frames | read (gap-durable 1.4) |
| R12 | Footer / status line | partly | partly | degraded | **degraded** | welcome.cwd, host_state.agent, list_models.context_window; no cost, % or branch | read (gap-durable 1.4) |
| R13 | Widgets | partly | none | degraded | **degraded** | status{key,text} defined, no emitter (protocol.ts:97) | read (gap-durable 1.4) |
| R14 | Header / loaded resources | partly | none | n/a | n/a | welcome frame (protocol.ts:83) | read (gap-durable 1.4) |
| R15 | Themes | yes | none | n/a | n/a | - | read (gap-durable 1.4) |
| R16 | Error / status / notice lines | partly | partly | degraded | **degraded** | notice{level,message} has two emitters (plan-review.ts:130,184; skills.ts:66) | read (gap-durable 1.4) |
| R17 | Custom messages / entries | partly | yes | stream-only | stream-only | entries kind cpd-chat, cpd-event, cpd-context ... | read (gap-durable 1.4) |
| R18 | Fullscreen viewport | yes | none | n/a | n/a | - | read (gap-durable 1.4) |
| R19 | Terminal title | partly | none | n/a | n/a | host_state.name could feed it | inferred (gap-durable 1.4) |
| D01 | ctx.ui.select | yes | none | ok | ok | ui_request select / ui_response value (protocol.ts:62-95,143-160) | read (gap-durable 1.5) |
| D02 | ctx.ui.confirm | yes | none | ok | ok | confirm kind defined; no host tool uses it | read (gap-durable 1.5) |
| D03 | ctx.ui.input | yes | none | ok | ok | input kind defined; no host tool uses it | read (gap-durable 1.5) |
| D04 | ctx.ui.editor | yes | none | ok | ok | used by plan review (plan-review.ts:137) with purpose other | read (gap-durable 1.5) |
| D05 | ctx.ui.notify | yes | none | ok | ok | notice frame (protocol.ts:96) | read (gap-durable 1.5) |
| D06 | setStatus / setWidget / setTitle | yes | none | degraded | **degraded** | status frame defined, no emitter | read (gap-durable 1.5) |
| D07 | setEditorText / pasteToEditor | yes | none | missing | **missing** | no kind; editor dialog has prefill (protocol.ts:93) | read (gap-durable 1.5) |
| D08 | ctx.ui.custom (overlay components) | yes | none | n/a | n/a | questionnaire is the one custom dialog, as data (protocol.ts:94) | read (gap-durable 1.5) |
| D09 | TUI-internal UI APIs | yes | none | n/a | n/a | - | read (gap-durable 1.5) |
| D10 | Dialog timeout | yes | none | ok | ok | ms -> expires_at and timer (held.ts:4,64-67); no cpd dialog sets ms | read (gap-durable 1.5) |
| N01 | Auto-compaction | yes | yes | degraded | **degraded** | default reserve, not cpd ratio; no toggle (harness.ts:99-103) | read (gap-durable 1.6) |
| N02 | Message history | yes | yes | ok | ok | history{before,limit} pages entries (run.ts:508-529) | read (gap-durable 1.6) |
| N03 | Session lifecycle hooks | yes | partly | n/a | n/a | only beforeTool and onYield used (extension.ts:16; run.ts:239) | read (gap-durable 1.6) |
| P01 | /reload auto-trigger after install | cpd | yes | degraded | **degraded** | reload wire exists; nothing triggers it after an install | read (gap-durable 1.8) |
| P02 | /plan-review (reopen pending review) | cpd | none | degraded | **degraded** | replay at attach works (server.ts:126-128); hint names a missing command (plan-review.ts:42) | read (gap-durable 1.8) |
| P03 | /quit marker | cpd | none | n/a | n/a | detach instead | read (gap-durable 1.8) |
| P04 | Compaction reserve ratio | cpd | yes | degraded | **degraded** | compactionPolicy has no caller (compaction.ts:26) | read (gap-durable 1.8) |
| P05 | Tier-driven model follow (retier) | cpd | yes | degraded | **degraded** | session-model-changed has no handler; client configure does not tell the engine (inferred) | read (gap-durable 1.8) |
| P06 | Mentions painting | cpd | none | missing | **missing** | no mentions code in host | read (gap-durable 1.8) |
| P07 | Typed-message reply flag | cpd | yes | ok | ok | submit with reply:true (run.ts:459) | read (gap-durable 1.8) |
| P08 | /compact-tools style picker | cpd | none | n/a | n/a | - | read (gap-durable 1.8) |
| P09 | /silent and ctrl+' | cpd | none | n/a | n/a | - | read (gap-durable 1.8) |
| P10 | Jump hint and question-header restyle | cpd | none | n/a | n/a | - | read (gap-durable 1.8) |
| P11 | Multi-question tab bar restyle | cpd | none | n/a | n/a | - | read (gap-durable 1.8) |
| P12 | Chat pad | cpd | none | n/a | n/a | - | read (gap-durable 1.8) |
| P13 | Editor box | cpd | none | n/a | n/a | - | read (gap-durable 1.8) |
| P14 | Input cursor | cpd | none | n/a | n/a | - | read (gap-durable 1.8) |
| P15 | Viewport keeper | cpd | none | n/a | n/a | - | read (gap-durable 1.8) |
| P16 | Compact tool rows (fold) | cpd | yes | stream-only | stream-only | no wire kind for tool rows | read (gap-durable 1.9) |
| P17 | Grouped rows (claude/codex styles) | cpd | none | n/a | n/a | default style is compact, so off by default | read (gap-durable 1.9) |
| P18 | Collapsible thinking | cpd | yes | stream-only | stream-only | - | read (gap-durable 1.9) |
| P19 | Compaction fold and "Compacting..." cue | cpd | yes | stream-only | stream-only | compact wire + stream entries | read (gap-durable 1.9) |
| P20 | Working label, loader, progress words | cpd | partly | degraded | **degraded** | host_state.state only; no progress text (status unused) | read (gap-durable 1.9) |
| P21 | Dango theme (16 indexed colours) | cpd | none | n/a | n/a | - | read (gap-durable 1.9) |
| P22 | Dango italics | cpd | none | n/a | n/a | - | read (gap-durable 1.9) |
| P23 | Balloons (user and agent bubbles) | cpd | none | n/a | n/a | whether entries carry a time is unverified (gap-durable 1.9) | inferred (gap-durable 1.9) |
| P24 | Pills | cpd | none | n/a | n/a | - | read (gap-durable 1.9) |
| P25 | Time display and zone line | cpd | none | degraded | **degraded** | zone line built once at build, stale until reload (run.ts:231; inferred) | inferred (gap-durable 1.9) |
| P26 | cpd message folds | cpd | yes | stream-only | stream-only | stored as kinds cpd-event, cpd-context ... (turn.ts:35-39) | read (gap-durable 1.9) |
| P27 | cpd-context notice | cpd | yes | stream-only | stream-only | prepareTurn writes the entry (turn.ts:49-94) | read (gap-durable 1.9) |
| P28 | Skills-changed notice | cpd | yes | ok | ok | skillsCheck + notice (skills.ts:44-67) | read (gap-durable 1.9) |
| P29 | ask_user_question schema, validation, G2 limit | cpd | yes | ok | ok | same schema and validator (tools.ts:35-58); G2 (guards.ts:45-47) | read (gap-durable 1.10) |
| P30 | ask_user_question answer kinds | cpd | none | degraded | **degraded** | questionnaire ui_request/ui_response (protocol.ts:65-70,153-157); answer cast, not validated (dialogs.ts:91) | read (gap-durable 1.10) |
| P31 | ask_user_question notes, global note, partial submit | cpd | none | degraded | **degraded** | UiQuestion has no notes channel | read (gap-durable 1.10) |
| P32 | ask_user_question tabs, preview pane, multi-select rows | cpd | none | ok | ok | questions delivered as data (protocol.ts:94); preview in UiQuestion is inferred | inferred (gap-durable 1.10) |
| P33 | ask_user_question collapse, displaced, hang-guard, bell | cpd | none | n/a | n/a | no inline slot on the host | read (gap-durable 1.10) |
| P34 | ask_user_question 2 h stuck bound and no_ui | cpd | partly | missing | **missing** | no ms on the questionnaire (tools.ts:52); held until a client attaches | read (gap-durable 1.10) |
| P35 | ask_user_question G4 label visibility | cpd | yes | degraded | **degraded** | visible: [] at build (run.ts:231, spot-checked): earlier chat never counts | read (gap-durable 1.10) |
| P36 | cpd_plan_review dialog | cpd | yes | degraded | **degraded** | select title is fixed "Approve the plan review", no review id or title (plan-review.ts:126) | read (gap-durable 1.10) |
| P37 | cpd_secret_approve dialog | cpd | yes | ok | ok | tools.ts:60-97 for both roles; TUI gives it to Plan Leads only | read (gap-durable 1.10) |
| P38 | cpd_host_approve dialog | cpd | yes | ok | ok | tools.ts:98-114 registered for every session, refuses non-Managers at execute | read (gap-durable 1.10) |
| P39 | MCP attach and mcp login | cpd | none | missing | **missing** | no MCP attach (run.ts:223-227, codemodeServers = []); mcp_login never emitted | read (gap-durable 1.10) |
| P40 | Dialog-state reporting | cpd | none | ok | ok | held_ui and state blocked (protocol.ts:55-60) | read (gap-durable 1.10) |
| P41 | Loop-guard visibility | cpd | yes | degraded | **degraded** | host_state ignores the trip (run.ts:545 vs wanted() :188-189) | read (gap-durable 1.11) |
| P42 | Agent-side guards (secret, hostapprove, main, noblock, G2) | cpd | yes | ok | ok | guards.ts:26-51 | read (gap-durable 1.11) |
| P43 | Hidden turn plumbing | cpd | yes | ok | ok | turn.ts:64-146; extension.ts:6-17; run.ts:162-167 | read (gap-durable 1.11) |
| P44 | Event delivery (follower, cursor, steer vs follow-up) | cpd | yes | degraded | **degraded** | session-message is followUp on host, steer in TUI (deliver.ts:32); model-changed, renamed, mcp-attached unhandled | read (gap-durable 1.12) |

### 4.2 Ranked gap list (make it work, then make it good; nothing measured for speed)

| Rank | Gap | Rows | Evidence | Fix size | Ev |
|---|---|---|---|---|---|
| 1 | Stream is a feature pi-durable 1.5.0 deleted; every `stream-only` row dies on upgrade | R01 R03 R06 R08 R10 R11 R17 P16 P18 P19 P26 P27 | `harness.ts:5,50,77`; 1.5.0 `CHANGELOG.md:49,56` | host migration, keep the 1.3.0 pin | read |
| 2 | G4 label check sees no chat (`visible: []`) so a Lead citing labels from earlier chat is refused | P35 | `run.ts:231`, `guards.ts:49`; 12 G4 refusals already on the TUI (`measured`, use-features (b)) | small, host only | read |
| 3 | A question or review held forever with no client: no `ms`, no `no_ui`, Session stays `blocked` | P34 P36 P40 | `tools.ts:52`, `server.ts:126-128`, `run.ts:189` | small, `ms` exists in `dialogs.ts:54` | read |
| 4 | Model changes drift from the engine: `session-model-changed` unhandled, `configure` refused while pending | S02 S04 P05 | `run.ts:469,616`; grep no handler (`measured`, gap-durable) | small, host handler | read |
| 5 | cpd compaction reserve ratio is dead code on the host | P04 N01 S20 | `compaction.ts:26` no caller; `harness.ts:99-103` | small, `settings` getter | read |
| 6 | `fork` ignores `at`, rejects `ownership` | S15 S16 | `run.ts:494-507` | small, honest rejection or pass-through | read |
| 7 | No tree navigation or branch summary | S03 R09 | `Conversation` interface lists no navigate; unopened hidden API is `inferred` | needs pi-durable change | inferred |
| 8 | `host_state.agent` empty until first `configure` | R07 | `run.ts:343,471,545` | one line | read |
| 9 | `host_state.state` ignores a loop-guard trip; held inputs invisible | P41 M04 | `run.ts:545` vs `:188-189`, `:377-380` | small | read |
| 10 | No user shell, `/new`, settings, queue modes, scoped models, login, export | C03 C04 S19 S01 M03 S05 S18 S06 | `KINDS` (`protocol.ts:114`) | small each; `/new` needs a product decision first | read |
| 11 | MCP attach and login unimplemented; 161 MCP calls in the corpus | P39 | `run.ts:223-227`; use-features (a) | medium host bridge | read |
| 12 | Dead or half-wired wire kinds: `status` no emitter, `waiting` never set, `notice` two emitters | M09 D06 R13 R16 P20 | gap-durable 2 item 12 | emit or delete | read |
| 13 | Zone line frozen at build until `reload` | P25 | `run.ts:231`, `extension.ts:14` | small | inferred |
| 14 | Questionnaire answers unvalidated, no notes channel | P30 P31 | `dialogs.ts:91`, `protocol.ts:155` | small, validate at the boundary | read |
| 15 | Plan-review dialog title generic, hint names a missing command, editor purpose `other` | P36 P02 D04 | `plan-review.ts:42,126,137` | small | read |
| 16 | All other rows are client work, not host gaps | R01-R06 R10-R17 P16-P27 | gap-durable 2 item 16 | UI | inferred |

## 5. Coupling classification

Rule (precedence applied top-down, so each row has exactly one class; `inferred` as a definition, the inputs are `read`):

| Class | Definition | Test | Ev |
|---|---|---|---|
| `durable-wire` | carried by durable-host protocol v1 today (a kind or frame exists, even if degraded) | marks `ok` or `degraded` with a named kind in section 4 | read |
| `rpc` | not on the wire, but a dedicated `pi --mode rpc` command or event carries it | inv-pi "exposed in rpc" is yes | read |
| `small-addition` | portable: the data is in the stream or a small message, or only UI is missing | the row names the smallest message or UI work | inferred |
| `tui-hard` | needs pi-coding-agent's TUI or process, or is chrome a browser replaces | inv-pi "only as TUI" list, inv-cpd `tui-patch`, no rpc | read |

- `rpc` rows are available only through pi-agent today; on the host they are `missing` or `degraded` until the named addition (`read`, section 4).
- `small-addition` includes UI-only rows (markdown, tool rows, folds); "small" is a size guess except where gap-durable states the fix (`guess`). P39 MCP bridge is medium, not small, and is flagged (`read`, gap-durable 2 item 11).

| Class | Baseline rows | cpd rows | Total | Ev |
|---|---|---|---|---|
| `durable-wire` | 20 | 15 | 35 | measured (counted from the 5.1 table) |
| `rpc` | 18 | 0 | 18 | measured |
| `small-addition` | 22 | 19 | 41 | measured |
| `tui-hard` | 27 | 10 | 37 | measured |

### 5.1 Table (one class per inventory row)

| ID | Feature | Class | Smallest addition | Ev |
|---|---|---|---|---|
| C01 | @file mention | `small-addition` | client picker; optional list_files wire over env.listFiles | inferred |
| C02 | Path completion (Tab) | `small-addition` | same as C01 | inferred |
| C03 | !cmd (bash into context) | `rpc` | host: user_bash{command,exclude} message that runs through env and writes an entry | read |
| C04 | !!cmd (bash excluded from context) | `rpc` | same message with exclude=true (silent write, model: []) | read |
| C05 | Bash-already-running guard | `small-addition` | UI state; only after C03 exists | inferred |
| C06 | Builtin slash dispatch (typed /x) | `tui-hard` | client maps each command to a wire kind (rows S01-S24) | read |
| C07 | Extension slash commands | `rpc` | host command list message; durable has no registerCommand | read |
| C08 | Prompt-template commands | `rpc` | client-side expansion before submit | read |
| C09 | Skill commands (/skill:name) | `rpc` | none needed: role skill is always loaded | read |
| C10 | Image paste / attach | `durable-wire` | pass createCodingTools({images}) at harness.ts:96 | read |
| C11 | Multi-line editing keys | `tui-hard` | none: browser textarea | read |
| C12 | Large-paste collapse | `tui-hard` | none (6 typed prompts exceed 2000 chars, measured) | read |
| C13 | Input history (Up/Down) | `small-addition` | UI derives history from history + stream; no host change | inferred |
| C14 | Autocomplete menu | `small-addition` | command/skill list message (get_commands analogue) | inferred |
| C15 | Keybindings config | `tui-hard` | none: browser shortcuts | read |
| C16 | External editor (Ctrl+G) | `tui-hard` | none: browser textarea | read |
| C17 | Double-Esc selector | `tui-hard` | none | read |
| C18 | Suspend / exit keys | `tui-hard` | none (detach exists) | read |
| C19 | Extension input transform | `rpc` | none: cpd has no transform | read |
| C20 | Extension shortcuts | `tui-hard` | none | read |
| S01 | /settings (+ settings selector) | `small-addition` | host-owned settings getter + settings message | inferred |
| S02 | /model (+ selector, Ctrl+P cycle) | `durable-wire` | drop or queue the pending guard | read |
| S03 | /tree (+ tree selector, navigation) | `tui-hard` | needs a pi-durable change for in-place navigation; fork-based read-only tree = UI + lift root-only | inferred |
| S04 | /thinking (+ selector, Shift+Tab cycle) | `durable-wire` | drop pending guard; cycle is UI | read |
| S05 | /scoped-models | `tui-hard` | none (settings store if ever wanted) | read |
| S06 | /export | `rpc` | UI exports from history | read |
| S07 | /import | `tui-hard` | none: engine creates sessions | inferred |
| S08 | /share | `tui-hard` | none (cpd has its own CLI) | read |
| S09 | /bug | `tui-hard` | none | read |
| S10 | /copy | `rpc` | UI copies | read |
| S11 | /name (rename) | `rpc` | handle session-renamed in the follower, push host_state | read |
| S12 | /session (stats) | `rpc` | UI computes from usage_changed + list_models.context_window | read |
| S13 | /changelog | `tui-hard` | none | read |
| S14 | /hotkeys | `tui-hard` | none | read |
| S15 | /fork (+ fork selector) | `rpc` | pass at to the engine or reject at | read |
| S16 | /clone | `rpc` | UI via fork at last entry | read |
| S17 | /trust (+ trust selector) | `tui-hard` | none | read |
| S18 | /login /logout (+ OAuth dialogs) | `tui-hard` | login dialog over existing select/input; OAuth browser step is client | read |
| S19 | /new | `rpc` | reset{handoff?} after a product decision | read |
| S20 | /compact [instructions] | `durable-wire` | report placed/stale outcome | read |
| S21 | /resume (+ session selector) | `durable-wire` | none | read |
| S22 | /reload | `durable-wire` | none | read |
| S23 | /quit | `tui-hard` | none | read |
| S24 | /llama, /debug, easter eggs | `tui-hard` | none | read |
| M01 | Steer (Enter while running) | `durable-wire` | none | read |
| M02 | Follow-up (Alt+Enter) | `durable-wire` | none | read |
| M03 | Queue modes (all / one-at-a-time) | `rpc` | host settings getter (low value) | read |
| M04 | Queued message display | `small-addition` | UI over inbox_update; held inputs in host_state | inferred |
| M05 | Dequeue to editor (Alt+Up) | `small-addition` | clear_queue returning the removed texts | inferred |
| M06 | Escape abort | `durable-wire` | finer abort per task (harness.abortTask) has no wire kind | read |
| M07 | Ctrl+C clear / quit | `tui-hard` | none | read |
| M08 | Queue during compaction | `durable-wire` | none | read |
| M09 | Working indicator | `durable-wire` | emit status or delete the dead kind | read |
| M10 | Auto-retry display and toggle | `rpc` | none (low value) | read |
| M11 | Terminal progress (OSC) | `tui-hard` | none | read |
| R01 | Streaming assistant text | `small-addition` | UI decodes message_update frames; no new wire | inferred |
| R02 | Markdown (+ Mermaid, LaTeX) | `small-addition` | UI only: a markdown renderer | inferred |
| R03 | Tool-call rendering | `small-addition` | UI decodes tool_execution_* frames | inferred |
| R04 | Tool output collapse / expand (Ctrl+O) | `small-addition` | UI only | inferred |
| R05 | Diffs | `small-addition` | UI computes from details | inferred |
| R06 | Thinking blocks (+ hide) | `small-addition` | UI only | inferred |
| R07 | Thinking level / model display | `durable-wire` | seed agentState from conversation.agent() at start | read |
| R08 | Compaction summary display | `small-addition` | UI only | inferred |
| R09 | Branch summary | `tui-hard` | follows S03 (pi-durable change) | inferred |
| R10 | Bash execution blocks | `small-addition` | UI only (user bash needs C03) | inferred |
| R11 | Images in output | `small-addition` | UI only | inferred |
| R12 | Footer / status line | `small-addition` | UI computes %; branch needs a git-read wire | inferred |
| R13 | Widgets | `small-addition` | emit status frames | inferred |
| R14 | Header / loaded resources | `small-addition` | UI only | inferred |
| R15 | Themes | `tui-hard` | none: CSS theme | read |
| R16 | Error / status / notice lines | `durable-wire` | surface cpd-error as notice | read |
| R17 | Custom messages / entries | `small-addition` | UI renders by kind | inferred |
| R18 | Fullscreen viewport | `tui-hard` | none: browser scrolling | read |
| R19 | Terminal title | `tui-hard` | none: document.title | inferred |
| D01 | ctx.ui.select | `durable-wire` | none | read |
| D02 | ctx.ui.confirm | `durable-wire` | none | read |
| D03 | ctx.ui.input | `durable-wire` | none | read |
| D04 | ctx.ui.editor | `durable-wire` | pass purpose plan_review | read |
| D05 | ctx.ui.notify | `durable-wire` | none | read |
| D06 | setStatus / setWidget / setTitle | `small-addition` | emit status or delete | inferred |
| D07 | setEditorText / pasteToEditor | `rpc` | composer-prefill frame, only if a Lead must prefill | read |
| D08 | ctx.ui.custom (overlay components) | `tui-hard` | none | read |
| D09 | TUI-internal UI APIs | `tui-hard` | none | read |
| D10 | Dialog timeout | `durable-wire` | none | read |
| N01 | Auto-compaction | `rpc` | see P04 | read |
| N02 | Message history | `durable-wire` | none | read |
| N03 | Session lifecycle hooks | `rpc` | none | read |
| P01 | /reload auto-trigger after install | `small-addition` | engine or host watcher sends reload | inferred |
| P02 | /plan-review (reopen pending review) | `durable-wire` | fix the hint text | read |
| P03 | /quit marker | `tui-hard` | none | read |
| P04 | Compaction reserve ratio | `small-addition` | Harness.open({settings:{get compaction(){...}}}) | inferred |
| P05 | Tier-driven model follow (retier) | `durable-wire` | handler + route client configure via cpd session retier | read |
| P06 | Mentions painting | `small-addition` | id feed (cpd session/plan/context list) + client painter | inferred |
| P07 | Typed-message reply flag | `durable-wire` | none | read |
| P08 | /compact-tools style picker | `tui-hard` | none | read |
| P09 | /silent and ctrl+' | `small-addition` | UI filter; no protocol change | inferred |
| P10 | Jump hint and question-header restyle | `tui-hard` | none | read |
| P11 | Multi-question tab bar restyle | `tui-hard` | none | read |
| P12 | Chat pad | `tui-hard` | none | read |
| P13 | Editor box | `tui-hard` | none | read |
| P14 | Input cursor | `tui-hard` | none | read |
| P15 | Viewport keeper | `tui-hard` | none | read |
| P16 | Compact tool rows (fold) | `small-addition` | UI rebuilds the fold from tool_execution_* frames | inferred |
| P17 | Grouped rows (claude/codex styles) | `small-addition` | UI only, optional | inferred |
| P18 | Collapsible thinking | `small-addition` | UI only | inferred |
| P19 | Compaction fold and "Compacting..." cue | `small-addition` | UI only | inferred |
| P20 | Working label, loader, progress words | `small-addition` | emit status frames with progress text | inferred |
| P21 | Dango theme (16 indexed colours) | `small-addition` | UI maps the 16 indices | inferred |
| P22 | Dango italics | `tui-hard` | none: CSS | read |
| P23 | Balloons (user and agent bubbles) | `small-addition` | UI only; check entry timestamps | inferred |
| P24 | Pills | `small-addition` | UI only | inferred |
| P25 | Time display and zone line | `small-addition` | make the section a getter over now() | inferred |
| P26 | cpd message folds | `small-addition` | UI renders by kind | inferred |
| P27 | cpd-context notice | `small-addition` | UI only | inferred |
| P28 | Skills-changed notice | `durable-wire` | none | read |
| P29 | ask_user_question schema, validation, G2 limit | `durable-wire` | none | read |
| P30 | ask_user_question answer kinds | `durable-wire` | validate the answer at the boundary | read |
| P31 | ask_user_question notes, global note, partial submit | `small-addition` | notes field in UiQuestion/answer | inferred |
| P32 | ask_user_question tabs, preview pane, multi-select rows | `durable-wire` | UI builds it | inferred |
| P33 | ask_user_question collapse, displaced, hang-guard, bell | `tui-hard` | none | read |
| P34 | ask_user_question 2 h stuck bound and no_ui | `small-addition` | pass ms 7 200 000; return no_ui when clients is 0 | inferred |
| P35 | ask_user_question G4 label visibility | `small-addition` | build visible from conversation.entries() at call time | inferred |
| P36 | cpd_plan_review dialog | `durable-wire` | put review id and title in the select title | read |
| P37 | cpd_secret_approve dialog | `durable-wire` | none | read |
| P38 | cpd_host_approve dialog | `durable-wire` | register by role (nit) | read |
| P39 | MCP attach and mcp login | `small-addition` | host MCP bridge as defineTools (medium, not small) + login dialog | inferred |
| P40 | Dialog-state reporting | `durable-wire` | none | read |
| P41 | Loop-guard visibility | `durable-wire` | hostState() uses wanted() | read |
| P42 | Agent-side guards (secret, hostapprove, main, noblock, G2) | `durable-wire` | none | read |
| P43 | Hidden turn plumbing | `durable-wire` | none | read |
| P44 | Event delivery (follower, cursor, steer vs follow-up) | `durable-wire` | host handlers for the three Events | read |

### 5.2 What the classes mean for the choice

- `tui-hard` cpd rows are all `tui-patch` chrome (pad, editor box, cursor, viewport keeper, hint restyle, italics, `/quit` marker, hang-guard) plus `/compact-tools`; none carries data, so a web UI loses no information by dropping them (`read`, inv-cpd 5 "Patches vs public API"; `inferred` for "no information").
- The pi-agent path keeps every `rpc` row but loses all rendering and dialog components under `--mode rpc`, and the adapter is inert, so cpd's guards, plan review and secret tools do not run (`read`, inv-cpd 0).
- The durable-host path already carries the cpd-specific dialogs (P29-P38) and the User's send, steer, abort, compact, configure, history and reload kinds; the work left is rendering and the degraded rows (`read`, gap-durable 1.10, 1.3).

## 6. Empirical usage

All tables are `measured`. Source tag per table: `UP` = use-prompts (242 sessions, cut at 2026-11-03T12:00Z); `UF` = use-features (266 loaded sessions, uncut). Scripts: `/tmp/pi-chat-features/prompts.py` and `/tmp/pi-chat-features/features.py`.

### 6.1 Corpus

| Measure | Value | Source | Ev |
|---|---|---|---|
| interactive sessions | 242 (lead 229, manager 13) | UP | measured |
| interactive sessions loaded | 266 (lead 251, manager 15) of 269 rows | UF | measured |
| span | 2026-10-19 to 2026-11-03 (about 16 days); by month 2026-10: 207, 2026-11: 35 | UP | measured |
| assistant turns per session (242) | median 69, p90 317, p99 1,122, max 3,117; total 34,678 | UP | measured |
| message entries per session (242) | median 146, p90 682, p99 2,402, max 6,316; total 71,422 | UP | measured |
| user-role messages per session (242) | median 1, p90 7, p99 49, max 209; total 976 | UP | measured |
| JSONL bytes per session (242) | median 806,517.5, p90 6,137,923, p99 16,893,082, max 25,492,019 | UP | measured |
| tool calls per session (266) | median 67, p90 278, p99 1,162, max 2,800; total 34,822, 29 distinct names | UF | measured |
| entry types (266) | compaction 286, context_edit 384, custom 4,258, custom_message 4,960, message 74,429, model_change 652, session 266, session_info 285, thinking_level_change 1,029 | UF | measured |
| message roles (266) | assistant 36,127, system 2,456, toolResult 34,811, user 1,035 | UF | measured |
| runner | pi only: 250 `by_user=0`, 19 `by_user=1`; no durable row | UF, UP | measured |

### 6.2 Prompts (749 typed, UP)

| Measure | Value | Share | Ev |
|---|---|---|---|
| typed prompts | 749 (manager 401 in 13 sessions, lead 348 in 76 sessions) | 100% | measured |
| sessions with zero typed prompts | 153 of 242 | 63.2% | measured |
| typed per session | median 0, p90 6, p99 49, max 209 | - | measured |
| length, chars | median 61, p90 254, p99 1,820, max 8,017; sum 113,163 | - | measured |
| length, lines | median 1, p90 3, p99 20, max 69 | - | measured |
| single line | 639 | 85.31% | measured |
| multi-line | 110 | 14.69% | measured |
| pasted block above 2,000 chars | 6 | 0.80% | measured |
| starts with `/` | 9, all file paths | - | measured |
| slash command (`^/name`) | 0 (also 0 of /reload /compact /mcp /plan-review /model /tree /resume /fork /new /name /session /export) | 0% | measured |
| starts with `!` or `!!` | 0 | 0% | measured |
| `image` content blocks | 0 | 0% | measured |
| image-file path as text (screenshot paths) | 17 | 2.27% | measured |
| expanded `<file name=` block (real `@file`) | 0 | 0% | measured |
| `@token` matches | 5 prompts, 11 matches, 10 distinct; all npm scopes or package names; real `@file` mentions 0 | - | measured |

- Zero means "not persisted", not "never used": pi's TUI handles slash commands and `!bash` without a `role: user` entry (`inferred`, use-prompts "Limits").
- Injected vs typed (UP, `measured`): 976 user-role = 227 `file_brief` + 749 `typed`; 4,657 `custom_message` entries are machine-injected: `cpd-context` 1,525, other cpd 1,540, peer messages 893, `cpd-plan-summary` 238, `cpd-plan-decision` 225, User-labelled engine nags 78, `cpd-skills` 71, User-labelled review requests 28, engine notices 28, `cpd-first-message` 15, `cpd-reminder` 12, other User-labelled 4. Machine text is 4,657 against 749 typed, about 6.2 to 1 (`measured` counts; ratio is arithmetic).
- Top openers (UP, `measured`): `.` 23, `bug report to` 6, `what are you` 5, `status` 5, `where were we` 4, `i dont see` 4, `tldr` 4, `why i see` 4, screenshot path 3, `are you able` 3, `do it` 3, `where are we` 3, `merged` 3, `what is the` 3, `homeserver install brokewith:` 3. The lone `.` is probably a continue nudge (`guess`).

### 6.3 Mid-turn steer (UP)

| Measure | Value | Ev |
|---|---|---|
| mid-turn (structural: previous entry is a `toolResult` or assistant `toolUse`) | 51 (6.81% of 749) | measured |
| idle | 698 (93.19%), 15 of them first messages | measured |
| structural vs timestamp cross-tab | mid and earlier 49; mid and later 2; idle and earlier 11; idle and later 687 | measured |
| agreement of the two methods | 736 of 749 (98.3%) | measured |
| upper bound | about 62 (51 + 11 final-answer overlaps) | inferred |
| gap, mid-turn (s) | median -5.741, p90 -0.757 | measured |
| gap, idle (s) | median 300.2, p90 6,720.9, p99 75,115.9 | measured |

### 6.4 Aborts (UP)

| Measure | Value | Ev |
|---|---|---|
| assistant `stopReason: aborted` | 8 of 34,678 turns (0.0231%) | measured |
| sessions with at least one | 5 of 242 (2.07%); max 3 per session | measured |
| strict interrupted `toolResult` | 1 (in 1 session) | measured |
| typed prompt directly after an aborted turn | 5 | measured |

### 6.5 `ask_user_question` (UF)

| Measure | Value | Ev |
|---|---|---|
| calls | 703 (695 with a result, 8 without) in 217 of 266 sessions (81.58%) | measured |
| calls per session, users only | median 2, p90 6, p99 13, max 86 | measured |
| questions per call (0 = unparseable) | 0: 10, 1: 356, 2: 162, 3: 101, 4: 74 | measured |
| questions single / multiSelect | 1,251 / 28 of 1,279 (multiSelect 2.19%) | measured |
| options per question | 2: 829, 3: 404, 4: 46 | measured |
| preview present | 99 options, 56 questions (4.38%), 54 calls (7.68%) | measured |
| call text chars (the G2 sum) | median 1,003, p90 1,551, p99 2,497, max 3,220 | measured |
| outcome per call | answered 559, cancelled 5, rejected before dialog 122, ui error result 9 | measured |
| rejected classes | G2 over 1,500 chars 75, G4 label stated nowhere 12, invalid JSON 10, header over 16 chars 12, label over 60 chars 8, missing required 5; ui_load_failed 9 | measured |
| G2-refused call size | median 1,750, p90 2,473, max 3,220 | measured |
| answers by kind (942 answered questions) | option 737 (78.2%), custom `Type something.` 185 (19.6%), multi 20 (2.1%) | measured |
| custom answer chars | median 77, p90 344, p99 1,152, max 1,361 | measured |
| multi answers with custom text / empty | 9 / 1 | measured |
| per-question notes / preview echoed / global notes | 1 / 34 / 0 | measured |
| partial submits (`unanswered`) | 1; cancels carrying partial answers 0 | measured |

Single vs multi-question, preview, outcome (calls, `measured`): multi-question no preview answered 219, cancelled 2, rejected 97, ui error 5; multi-question with preview answered 11, rejected 7; single no preview answered 299, cancelled 3, rejected 12, ui error 4; single with preview answered 30, rejected 6.

- 337 of 703 calls are multi-question and 74 use all four tabs (`measured` counts: 162+101+74); the free-text row carries 19.6% of answers (`inferred` reading).

### 6.6 Plan review and approvals (UF)

| Measure | Value | Ev |
|---|---|---|
| `cpd_plan_review` calls | 301 in 193 sessions; per session median 1, p90 3, p99 6, max 7 | measured |
| decisions (`cpd-plan-decision`) | APPROVED 240, REJECTED 0 | measured |
| decisions with a `Feedback from the reviewer:` block | 1 (APPROVED, 254 chars) | measured |
| `cpd-plan-review` entries by status | approved 241, pending 252, superseded 10 | measured |
| db `plan_reviews` | approved 241, rejected 0, superseded 11; 240 of 240 decisions matched, 0 mismatches | measured |
| `cpd_secret_approve` / `cpd_host_approve` calls | 0 / 0 | measured |
| bash text mentioning secret_approve / host_approve | 9 / 8 (prose, grep output, generated spec text, not calls) | measured |

### 6.7 Tools (UF, 34,822 calls)

| Rank | Tool | Calls | Sessions using | Share of sessions | Ev |
|---|---|---|---|---|---|
| 1 | `bash` | 32,010 (91.9% of calls) | 266 | 100.0% | measured |
| 2 | `read` | 1,085 | 261 | 98.12% | measured |
| 3 | `ask_user_question` | 703 | 217 | 81.58% | measured |
| 4 | `cpd_plan_review` | 301 | 193 | 72.56% | measured |
| 5 | `codemode` | 267 | 20 | 7.52% | measured |
| 6 | `write` | 157 | 37 | 13.91% | measured |
| 7 | `tool_search` | 76 | 20 | 7.52% | measured |
| 8 | `edit` | 59 | 10 | 3.76% | measured |
| - | MCP atlassian / slack / context7 | 78 / 80 / 3 | 5 / 14 / 2 | - | measured |

- `isError:true` results: `bash` 1,066, `ask_user_question` 122, `cpd_plan_review` 50, `read` 20, `codemode` 8, `edit` 6, `write` 6 (`measured`).
- Calls inside `codemode` (regex over the code text, `measured`): `bash` 372, `read` 118, `write` 29, Atlassian 118 across 14 tool names, Figma 4, Slack 5.
- Tool result sizes (bytes of JSON content, `measured`): all results median 1,474, p90 8,522, p99 50,109, max 943,541; assistant messages median 2,713, p90 21,530, p99 100,979, max 278,485. Total `bash` result bytes 102,448,515 of 134,429,523.

### 6.8 Compaction (UF)

| Measure | Value | Ev |
|---|---|---|
| compaction entries | 286 in 96 sessions; per session median 2, p90 6, p99 33, max 33 | measured |
| `tokensBefore` | median 201,647.5, p90 215,423, p99 280,851, max 592,228 | measured |
| summary bytes | median 6,704, p90 14,963, p99 27,227, max 34,067; sum 2,341,192 | measured |
| `systemMessage` checkpoint bytes | median 22,938, p90 26,237, p99 73,441; sum 6,773,998 | measured |
| summary-generation usage | median 59,031 totalTokens; 16.3595 total cost | measured |
| `fromHook` | true 280, false 6 | measured |
| manual vs auto | not recorded in the entry | measured |

### 6.9 Model and thinking changes (UF)

| Measure | Value | Ev |
|---|---|---|
| `model_change` entries | 652 in 266 sessions: 266 first-in-session, 386 transitions; 6 distinct models | measured |
| transitions naming the same model | 342 of 386 (bifrost sonnet 191, copilot sonnet 88, copilot opus-5.5 46, gemini 17) | measured (sum of the transition table) |
| transitions to a different model | 44 | measured (386 - 342) |
| `session-model-changed` db events | 35 (22 sessions) | measured |
| `thinking_level_change` | 1,029: medium 382, xhigh 329, high 317, max 1 | measured |
| assistant messages by `thinkingLevel` | xhigh 26,994, high 8,244, medium 808, max 80 | measured |
| forks | 0 on interactive sessions (6 db forks are Workers); `branch_summary` 0; branch points 0 | measured |
| `cpd mcp attach` bash calls | 11 in 6 sessions (atlassian 3, figma 2, slack 6); `mcp-attached` db events on interactive sessions 44 | measured |
| `/mcp login` | 2 in user messages; 18 in bash text; 7 sessions with either | measured |

### 6.10 Thinking bytes (UF)

| Measure | Value | Ev |
|---|---|---|
| assistant messages with at least one thinking block | 26,464 of 36,127 (73.25%) | measured |
| thinking blocks / empty | 28,879 / 554 | measured |
| block text bytes | median 446, p90 6,720, p99 29,534, max 73,190; total 71,230,004 | measured |
| `thinkingSignature` bytes per block | median 2,036, p90 18,764, p99 76,800, max 202,728; total 213,706,628 | measured |
| signature share of loaded JSONL bytes | 213,706,628 of 543,901,014 (39.3%) | measured (arithmetic) |

### 6.11 `cpd-context` notices (UF)

| Measure | Value | Ev |
|---|---|---|
| notices | 1,597 in 218 of 266 sessions | measured |
| bytes per notice | median 474, p90 2,769, p99 8,763, max 20,063 | measured |
| per session, users only | count median 3, p90 11, p99 83, max 338; bytes median 2,802, p90 13,040, p99 95,293, max 520,097 | measured |
| `custom_message` by type | cpd 2,733; cpd-context 1,597; cpd-plan-summary 252; cpd-plan-decision 240; cpd-skills 105; cpd-first-message 19; cpd-reminder 14 | measured |
| `custom` extension state entries | plannotator 3,534; cpd-plan-review 503; cpd-skills 214; cpd-loop-guard 4; codemode-store 3 | measured |

### 6.12 Largest tool results and images (UF)

| Bytes | Tool | Session | Entry | Ev |
|---|---|---|---|---|
| 943,541 | read | 55c99948-c6c6-45cf-91a9-7a28ff509f8e | 2f07bf1d | measured |
| 431,181 | codemode | a7dfd149-706f-4b72-b5f7-b85323079cda | 14df124c | measured |
| 408,115 | read | 0a96be92-c003-463e-bbdc-32115da7da71 | 9f45fa85 | measured |
| 402,688 | read | 55c99948-c6c6-45cf-91a9-7a28ff509f8e | 5435c495 | measured |
| 402,632 | read | c7297376-9a76-4f7a-ba7d-74d48cd2b9c4 | fe8bbbe5 | measured |
| 250,230 | read | 55c99948-c6c6-45cf-91a9-7a28ff509f8e | 76da6e3f | measured |
| 212,200 | read | b510fb80-05eb-442b-92fb-dedd316b73f9 | 8b94e580 | measured |
| 149,688 | read | 55c99948-c6c6-45cf-91a9-7a28ff509f8e | 633d5368 | measured |
| 72,566 | write | 453264f4-30ea-4bb4-bf62-38bcac9f9a21 | 49c45dc0 | measured |
| 71,076 | read | 0a96be92-c003-463e-bbdc-32115da7da71 | f1d98d29 | measured |
| 69,712 | read | 0a96be92-c003-463e-bbdc-32115da7da71 | ac01623c | measured |
| 61,996 | read | 55c99948-c6c6-45cf-91a9-7a28ff509f8e | e8e6001d | measured |
| 60,964 | read | edf9d810-de8c-4f08-a77d-db3bcd367db1 | 7cf67d0f | measured |
| 58,801 | bash | ca6579f0-6c90-43e1-b48f-f8f934063647 | 2c71bc04 | measured |
| 58,262 | bash | c7297376-9a76-4f7a-ba7d-74d48cd2b9c4 | e7fb1dfa | measured |

- Images (UF, `measured`): image blocks in tool results `read` 30 and `codemode` 1; in user, custom and assistant messages 0; 9 sessions with any image; base64 chars per tool-result image median 37,768, max 943,328.

### 6.13 db-vs-JSONL mismatches (UF cross-check)

| Check | db | JSONL | Verdict | Ev |
|---|---|---|---|---|
| plan reviews | approved 241, rejected 0, superseded 11 | 240 APPROVED decisions, 0 REJECTED, 241 `cpd-plan-review` approved | decisions matched 240 of 240; db counts all plans | measured |
| plan-review events | 504 total, 0 attributed to interactive sessions | n/a | an event per state change, not per dialog | inferred |
| `ask_user_question` | no table; `escalations` (answered 481, expired 46) are Worker asks | 703 calls | not comparable | measured |
| `mcp-attached` | 59 total, 44 on interactive sessions | 11 `cpd mcp attach` bash calls | **mismatch**: about 4x more attaches than bash calls | measured |
| model changes | `session-model-changed` 35 | `model_change` 652 (266 start entries, 386 later) | **mismatch**: events record cpd-driven switches only | measured |
| forks | `forked_from` 0 on interactive rows | `parentSession` 0, `branch_summary` 0 | agree | measured |
| session-message | 1,062 events, all to interactive sessions | `cpd` custom_message 2,733 | different grain | inferred |

## 7. Priority ranking for cpd-webui

Ranking rule (`inferred`): make it work first, so anything a Lead or Manager cannot proceed without is P0; P1 is measured but recoverable; P2 is unused or cosmetic. "If skipped" states what degrades. Every row cites a measured number (section 6) and the coupling class of section 5.

### P0 (the chat is unusable or Leads stall without it)

| # | Item (rows) | Measured number | Class | If skipped | Ev |
|---|---|---|---|---|---|
| P0-1 | Submit text with `when_busy` (M01 M02, P07) | 749 typed prompts, 85.31% single line, median 61 chars (UP 6.2) | `durable-wire` | the User cannot talk to a Lead or Manager | measured |
| P0-2 | Streaming assistant text and markdown (R01 R02) | 36,127 assistant messages; 34,678 turns in 242 sessions (UF, UP 6.1) | `small-addition` | no reading of replies; UI over untyped frames | measured |
| P0-3 | Tool rows with default-collapsed, truncated output (R03 R04 R10, P16) | 34,822 calls, `bash` 91.9%; results p99 50,109 B, max 943,541 B (UF 6.7) | `small-addition` | a 943 kB `read` result is rendered whole; page freeze (`inferred`) | measured |
| P0-4 | `ask_user_question` questionnaire (P29 P30 P32) | 703 calls in 217 of 266 sessions (81.58%); 559 answered; 74 four-question calls (UF 6.5) | `durable-wire` | Leads block on the tool they use most after `bash` and `read` | measured |
| P0-5 | Free-text answer row and G4 label fix (P30 P35) | 185 of 942 answers custom (19.6%); 12 G4 refusals on the TUI, on the host every call citing labels stated only in earlier chat is refused (`run.ts:231`) | `durable-wire` + `small-addition` | a Lead citing "X3" is refused every time; 1 in 5 answers unanswerable | measured |
| P0-6 | Plan review Approve path (P36) | 301 calls in 193 sessions; 240 APPROVED (UF 6.6) | `durable-wire` | no Plan Lead can finish planning | measured |
| P0-7 | Attach, history paging, snapshot (N02 S21) | median 69 and p99 1,122 assistant turns per session; 153 of 242 sessions have 0 typed prompts (UP 6.1, 6.2) | `durable-wire` | long or Lead-only sessions cannot be opened or resumed | measured |
| P0-8 | cpd message folds and `cpd-context` (P26 P27) | 4,657 injected `custom_message` against 749 typed (6.2:1); 1,597 `cpd-context` in 218 sessions (UP 6.2, UF 6.11) | `small-addition` | most of the transcript is unreadable machine text | measured |
| P0-9 | Working and blocked state (M09 P40) | 703 ask calls and 301 reviews each hold a Session `blocked` while open; 122 refusals precede the dialog (UF 6.5, 6.6) | `durable-wire` | the User cannot see why a Lead is waiting | inferred |

### P1 (measured use, recoverable)

| # | Item (rows) | Measured number | Class | If skipped | Ev |
|---|---|---|---|---|---|
| P1-1 | Abort (M06) | 8 aborted turns of 34,678 (0.0231%) in 5 of 242 sessions (UP 6.4) | `durable-wire` | runaway runs cannot be stopped; rare but no other control (`inferred`) | measured |
| P1-2 | Steer with queue display (M04 M05) | 51 mid-turn prompts, 6.81%, about 62 at most (UP 6.3) | `small-addition` | steering works but the User cannot see or withdraw a queued message | measured |
| P1-3 | Compaction fold, cue and cpd ratio wiring (P19 P04 N01 S20) | 286 compactions in 96 sessions; median `tokensBefore` 201,647.5 (UF 6.8) | `small-addition` | compactions arrive unannounced and at pi-durable's default reserve | measured |
| P1-4 | Thinking fold, no signature rendering (R06 P18) | 73.25% of assistant messages have thinking; signatures are 39.3% of bytes (UF 6.10) | `small-addition` | transcripts several times longer; signature bytes shipped for nothing | measured |
| P1-5 | Footer: model, thinking, context %, cost (R12 R07, S12) | median compaction at 201,647 tokens; 652 `model_change`, 1,029 `thinking_level_change` (UF 6.8, 6.9) | `small-addition` | the User cannot see the context window filling | measured |
| P1-6 | MCP attach in the host (P39) | 161 MCP calls (atlassian 5, slack 14, context7 2 sessions); 44 `mcp-attached` events on interactive sessions (UF 6.7, 6.13) | `small-addition` (medium) | Jira, Slack and Context7 are unavailable to durable Leads | measured |
| P1-7 | Plan review Change editor (P36 D04) | 0 REJECTED, 1 approval with feedback (UF 6.6) | `durable-wire` | untested path; a Lead cannot be redirected from review (`inferred`) | measured |
| P1-8 | Host dialog hardening: `ms` bound, `no_ui`, answer validation (P34 P30) | 5 cancelled calls, 9 ui error results (UF 6.5) | `small-addition` | an unanswered question holds the Session `blocked` forever | measured |
| P1-9 | Model/thinking selector and `session-model-changed` (S02 S04 P05) | 386 transitions, 44 to a different model; 35 engine events (UF 6.9) | `durable-wire` | the engine and host disagree on the model | measured |

### P2 (unused, cosmetic or rare)

| # | Item (rows) | Measured number | Class | If skipped | Ev |
|---|---|---|---|---|---|
| P2-1 | Slash-command menu and autocomplete (C06 C07 C14, S-rows) | 0 persisted slash commands in 749 prompts (UP 6.2); undercount `inferred` | `tui-hard` / `small-addition` | `/compact`, `/reload` reachable only as buttons | measured |
| P2-2 | User shell `!cmd` and `!!cmd` (C03 C04) | 0 of 749 start with `!` (UP 6.2); undercount `inferred` | `rpc` | the User asks the Lead to run commands, as the 32,010 `bash` calls already do | measured |
| P2-3 | `@file` picker and path completion (C01 C02) | 0 real `@file` mentions; 17 pasted screenshot paths (UP 6.2) | `small-addition` | paths are typed or pasted | measured |
| P2-4 | Image attach and inline images (C10 R11) | 0 user image blocks; 31 tool-result images in 9 sessions (UF 6.12) | `durable-wire` / `small-addition` | screenshots go as path text, as today | measured |
| P2-5 | Large-paste collapse, history, multi-line keys (C11 C12 C13) | 6 prompts above 2,000 chars (0.80%); 14.69% multi-line (UP 6.2) | `tui-hard` | a browser textarea suffices | measured |
| P2-6 | Fork, clone, tree, branch summary (S03 S15 S16 R09) | 0 forks, 0 branch points on interactive sessions (UF 6.9) | `tui-hard` / `rpc` | nothing observed; fork defect (`run.ts:499`) stays latent | measured |
| P2-7 | `cpd_secret_approve` and `cpd_host_approve` dialogs (P37 P38) | 0 calls each (UF 6.6) | `durable-wire` | the generic `select` already works; no extra UI | measured |
| P2-8 | `/new`, settings, queue modes, scoped models, login, share, export (S01 S05 S06 S18 S19 M03) | 0 persisted commands; no interactive-session use observable (UP 6.2) | `small-addition` / `rpc` / `tui-hard` | decide per row when asked for | measured |
| P2-9 | Mentions painting (P06) | no count exists; 893 peer messages carry ids (UP 6.2) | `small-addition` | ids shown as plain text | guess |
| P2-10 | TUI chrome: balloons, pills, pad, cursor, viewport, themes, Dango, silent mode (P09-P15 P21-P24 R15 R18) | no measurable signal in a transcript | `tui-hard` / `small-addition` | a different look, no lost information | guess |
| P2-11 | Loop-guard visibility (P41) | 4 `cpd-loop-guard` entries in 266 sessions (UF 6.11) | `durable-wire` | a tripped Lead looks idle | measured |
| P2-12 | Grouped rows, `/compact-tools`, `/silent` (P08 P09 P17) | default style is `compact`, so grouped rows are off (`read`, `NOTICE`); no usage signal | `tui-hard` / `small-addition` | none | guess |

## 8. Open questions and unverified claims

| # | Item | Label |
|---|---|---|
| 1 | Whether the User uses `/model`, `/compact`, `/reload` or `!bash`: pi's TUI persists none of them; 0 is an undercount. A cheap way to measure is a TUI hook that logs submitted commands (not built) | inferred |
| 2 | Whether the 10 unclassified JSONL files under `--Users-Clayton.Duarte--` are earlier or non-cpd pi sessions; they were not read | inferred |
| 3 | `rpc` behaviour of `setWorkingMessage`, `setWidget`, `custom`, and of a builtin slash command sent via `prompt` (sent to the model as text): not run | inferred |
| 4 | Whether the rpc build honours `enableSkillCommands`; whether `switch_session` accepts paths outside the sessions directory | inferred |
| 5 | Whether `cpd session fork` accepts an entry position; whether the cpd client lists sessions | guess |
| 6 | Whether entries carry a timestamp the Balloons row needs (`EntryRecord` not opened) | guess |
| 7 | pi-durable 1.4.0 was not unpacked; its watch surface comes from `docs/research/ratatui-chat-durable-ui.md`; 1.5.0 migration cost not measured | inferred |
| 8 | The diff in stream for R05 (tool result `details`) was never opened in gap-durable | inferred |
| 9 | Durable-host numbers (client count, latency, stream size) do not exist: no cpd client of the host was run, and no interactive session ran on it | read |
| 10 | The `small-addition` size labels are guesses except where gap-durable names the fix | guess |
| 11 | 3 interactive rows (2 `file` NULL, 1 file missing) are unmeasured in both artefacts | measured |
| 12 | Speed was not measured anywhere in this report; render cost claims are `inferred` from byte sizes in 6.7, 6.10, 6.12 | inferred |
