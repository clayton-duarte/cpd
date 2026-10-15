# Research: driving pi headless to replace pi's TUI pane with our own chat view

Job `research-pi-headless`, Workflow `cpd-ui`. Written against pi **1.2.0**. Every pi claim cites
`docs/<file>.md` § section, relative to
`~/.pi/agent/install/releases/1.2.0/node_modules/@earendil-works/pi-coding-agent/`. **Guess:** marks what I infer and did
not verify. Effort is in Jobs and Worker-hours; every effort figure is a guess.

## 1. Summary

**Recommendation: (a), a Rust/ratatui chat view in `cpd-tui` that drives `pi --mode rpc`, built in phases.** Keep
today's tmux chat pane (c) as the default until the new view has the must-haves listed in §4. Do not build a Node
sidecar (b).

- **Why it works.** RPC mode covers everything a Lead chat needs, as one JSONL protocol: prompt, steer, follow-up,
  abort, model and thinking, compaction, resume, fork, clone, naming, stats. Extension dialogs come through as a
  request/response sub-protocol. The scratch run in §3 shows every one of these working. RPC is language-independent,
  so we keep the User's choices: Rust, ratatui, one binary, and pi as a system dependency (vision "Principles", decision 9).
- **Effort, guess.**
  - **MVP** (transcript, input, send, stream, steer, interrupt, extension dialogs, one Lead): **5 Jobs, about 18–24
    Worker-hours.**
  - **Parity** with the must-have and nice rows of §4: **6 more Jobs, about 22–30 Worker-hours.**
  - **Total: about 11 Jobs, 40–55 Worker-hours.**
  - **The most costly parts:** the input editor and Markdown/tool rendering (§4), then owning the pi processes (§5).
- **Main risks:**
  1. **The cpd adapter does nothing outside the TUI.** `pi/extension.ts:176` returns from `session_start` when
     `ctx.mode !== "tui"`, so in RPC mode the adapter's queue delivery and `cpd_plan_review` tool never start. RPC sets
     `ctx.mode` to `"rpc"` and `ctx.hasUI` to `true` (`rpc-extension-ui.md` § Limitations). The fix is in `pi/`, outside
     this Workflow's `tui/` scope. It needs its own Job, and someone must first learn why the guard exists. Guess: it
     keeps the Lead adapter out of print and JSON runs, and `ctx.hasUI` would serve the same purpose.
  2. **Who owns the pi process.** Today pi lives in a tmux pane, so it survives cockpit restarts. An RPC child dies with
     its parent, and only one client can read its stdout. If that reader stops, pi stalls (`rpc.md` § Framing). Restarting
     or upgrading the view must not abort a Lead mid-turn, so a long-lived host process is needed (Phase 2, §6). It must
     be the only owner of each session file, because a second pi on the same file corrupts it (Spec SAFETY).
  3. **pi's TUI-only features have no RPC equivalent:**
     - `ctx.ui.custom()` returns `undefined`, and widgets are strings only (`rpc-extension-ui.md` § Limitations);
     - built-in commands such as `/settings`, `/tree`, `/resume` and `/login` do not run through `prompt`
       (`rpc-commands.md` § get_commands);
     - there is no tree-navigation command: `get_tree` reads the tree, and only `fork` and `clone` act on it
       (`rpc-commands.md` § Session, also checked in `dist/modes/rpc/rpc-types.d.ts`).

     So extension tool renderers and pi's `/tree` are lost. `rpiv-ask-user-question` already falls back to the
     select/input dialogs over RPC (its `ask-user-question.ts` checks `ctx.mode === "rpc"`).
  4. **Protocol drift.** The wire is pi's to change. Pin the pi version the view supports. Parse records with typed
     serde at the boundary, show an unknown event as a raw line, and fail loud on a malformed known event.
- **Gains:**
  - one visual language with the cockpit (the `research-components` Job);
  - cpd-native widgets inline, such as Job cards and plan-review approvals where plannotator's custom UI cannot reach;
  - switching sessions by re-pointing the view instead of `swap-pane`;
  - the cockpit's "needs you" dot read directly from `agent_settled` and the tool events, not through tmux options;
  - a protocol that headless Workers can reuse later.

## 2. pi's headless surfaces

Four ways to run pi without its TUI (`cli-integration.md` § Choose a mode; `sdk.md`):

| Surface | Boundary | Lifetime | Input after start | Fit for a chat view |
| --- | --- | --- | --- | --- |
| Print (`-p`) | child process | one invocation | none | no: final text only (`cli-integration.md` § Print to stdout) |
| JSON (`--mode json`) | child process | one invocation | none | no: events, but no later commands (`cli-integration.md` § Stream JSON events) |
| **RPC (`--mode rpc`)** | child process | long-lived | JSONL commands on stdin | **yes** |
| SDK (`createAgentSession`) | in-process Node/Bun | the host's | TypeScript calls | yes, but only from TypeScript |

### 2.1 RPC mode (`rpc.md`, `rpc-commands.md`, `rpc-extension-ui.md`, `json.md`)

**Protocol.**
- **Records.** JSONL in both directions. There are four record families: commands on stdin; `response`, session
  events, and extension UI records on stdout (`rpc.md` § Protocol records).
- **Correlation.** An optional string `id` on a command is echoed in its `response`. Handling is asynchronous, so
  correlate by id, not by order (`rpc.md` § Correlate commands and responses).
- **Framing.** Split on LF only, and never on U+2028/2029, which rules out Node's `readline`. Read stdout continuously,
  or pi stalls. stderr carries diagnostics and is not protocol (`rpc.md` § Framing).
- **Errors.** A failed command gets `success:false` with an `error` field. Malformed JSON gets a `command:"parse"`
  response with no id. A provider failure after a prompt was accepted appears only in the event stream (`rpc.md` §
  Errors). Closing stdin shuts pi down in order (`rpc.md` § Shutdown).
- **Rust implications.** A `BufRead::read_until(b'\n')` reader fits the framing. The TypeScript types live in
  `rpc-types.ts`, and pi ships `RpcClient` for TypeScript (`rpc.md` § Reference).

**Sessions: create, resume, fork.**
- **At startup** (`cli.md` § Sessions), the normal CLI flags apply:
  - `--session <path|id>` resumes;
  - `--fork <path|id>` (with `--session-id` to choose the new id) forks, which is what `fork-lead.sh` does today;
  - `--no-session` keeps the session in memory;
  - `--session-dir` chooses the storage;
  - `--name` names the session.
- **At runtime:**
  - `new_session` (optionally with `parentSession`) and `switch_session {sessionPath}`; an extension can cancel either
    (`rpc-commands.md` § new_session, § switch_session);
  - `fork {entryId}` from a user message (list the candidates with `get_fork_messages`) and `clone` of the active
    branch (`rpc-commands.md` § fork, § clone);
  - `set_session_name` (§ set_session_name).
- **Reading state:**
  - `get_state` gives `sessionFile`, `sessionId`, `sessionName`, the model, the thinking level, the queue modes and
    `isStreaming`;
  - `get_messages` gives the active-branch transcript;
  - `get_entries {since}` reads the whole append-only tree from a durable cursor "even across client restarts", the hook
    for reconnecting a view;
  - `get_tree` gives the tree with its labels (`rpc-commands.md` § State, § get_entries, § get_tree).
- **What RPC does not have:** a session list or picker, and moving the leaf inside one file (pi's `/tree`). Guess: the
  cockpit lists sessions itself (it does already), and `/tree` is out of scope.

**Streaming** (`json.md` § Event sequence, § Message events, § Reconstruct streaming messages).
- **Lifecycle events:** `agent_start`, `turn_start`, `message_start`, `message_update`*, `message_end`, `turn_end`,
  `agent_end`, then `agent_settled`.
- **`message_update` carries deltas only.** Each one holds an `assistantMessageEvent`: `text_*`, `thinking_*` or
  `toolcall_*`, with `start`, `delta` and `end` forms, keyed by `contentIndex`.
- **Reconstruction.** Build blocks from the deltas, replace each block with the `*_end` content, then replace the whole
  message with `message_end.message`.
- **Tools** report through `tool_execution_start`, `_update` (`partialResult`) and `_end` (`result`, `isError`,
  `durationMs`), keyed by `toolCallId` (`json.md` § Tool execution events).
- **Message shapes.** Messages use the shared types (`message-types.md`): User, Assistant (`stopReason`, `usage`),
  ToolResult, BashExecution, Custom (cpd's `cpd-plan-decision` is one, `pi/extension.ts:74`), BranchSummary and
  CompactionSummary.
- **`agent_settled` is the "done" signal.** `agent_end` can still be followed by retries, compaction or queued messages
  (`rpc.md` § Run lifecycle).

**Interrupt, steer, follow-up** (`rpc-commands.md` § Prompting).
- **`prompt`.** During a run, `prompt` must name `streamingBehavior: "steer" | "followUp"`, or it fails. The response's
  `disposition` is `started`, `queued` or `handled`; `handled` means an extension command consumed the prompt, so no run
  starts.
- **`steer`** is delivered after the current turn's tool calls, before the next model call.
- **`follow_up`** is delivered when the agent would otherwise stop.
- **Queue modes.** `set_steering_mode` and `set_follow_up_mode` choose `all` or `one-at-a-time` (the default) (§ Queue
  modes). The `queue_update` event carries the whole queue (`json.md` § Queue and state events).
- **`abort`** waits until pi is idle before it responds.
- **pi's Esc behaviour, recreated:** send `clear_queue` and put the text it returns back in the editor, then `abort`
  (§ clear_queue).
- **Extension commands** (`/name`) run immediately, even mid-run. Skill commands and prompt templates are expanded
  (§ prompt).

**Model and thinking** (`rpc-commands.md` § Model, § Thinking).
- **Model commands:** `get_available_models`, `set_model {provider, modelId}` and `cycle_model`.
- **Thinking commands:** `get_available_thinking_levels`, `set_thinking_level`, `cycle_thinking_level`, and the
  `thinking_level_changed` event (`json.md` § Queue and state events).
- **At startup:** `--model` and `--thinking` (`cli.md` § Models).

**Compaction** (`rpc-commands.md` § Compaction; `json.md` § Compaction events; `compaction.md`).
- **Commands:** `compact {customInstructions?}` returns the summary and token counts; `set_auto_compaction` turns it on
  or off.
- **Events:** `compaction_start {reason: manual|threshold|overflow}` and `compaction_end {result|aborted|errorMessage,
  willRetry}`.
- **Retries:** `summarization_retry_*` (compaction and branch summaries) and `auto_retry_start`/`_end` (turns).
- **Context usage:** `get_session_stats.contextUsage` gives the footer's context percentage (`rpc-commands.md` §
  get_session_stats).

**Extensions** (`extensions.md` § UI and modes; `rpc-extension-ui.md`).
- **Loading.** Extensions load in RPC mode. `ctx.mode` is `"rpc"` and `ctx.hasUI` is `true`.
- **Dialogs** (`select`, `confirm`, `input`, `editor`) arrive as `extension_ui_request {id, method, …, timeout?}`. They
  block the extension until we send `extension_ui_response {id, value | confirmed | cancelled}`. When a `timeout` runs
  out, pi resolves the dialog itself.
- **Fire-and-forget requests:** `notify`, `setStatus`, `setWidget` (string lines only), `setTitle`, `set_editor_text`.
- **Missing or degraded** (§ Limitations):
  - `custom()` returns `undefined`;
  - `setFooter`, `setHeader`, `setEditorComponent`, `addAutocompleteProvider`, `setWorkingMessage` and
    `setToolsExpanded` are no-ops;
  - `getEditorText()` returns `""`;
  - themes are unavailable.
- **Errors.** `extension_error` reports a handler that threw (`json.md` § RPC-only events).

Our three extensions under RPC (read from their source, not run):

| Extension | Under `--mode rpc` | Work needed |
| --- | --- | --- |
| cpd adapter, `~/projects/cpd/pi/extension.ts` | **Inert.** `session_start` returns when `ctx.mode !== "tui"` (line 176), so the queue watcher, `cpd_plan_review` and plan decisions never start. It needs only `ctx.ui.notify` (line 178), which RPC forwards. | Change the guard to `ctx.hasUI` or `mode in {tui, rpc}`: a Job in `pi/`, outside `cpd-ui`. |
| `tui/scripts/tmux-agent-state.ts` | **Inert.** It returns without `TMUX_PANE` and `CPD_TUI_SOCKET` (lines 14–16), because it publishes to a tmux pane. | Replace it: the chat host sees the same events (`agent_start`, `agent_settled`, `tool_execution_*`, `session_info_changed`) on the RPC stream and publishes "needs you" itself. |
| plannotator (`@plannotator/pi-extension`) | **Works, guess.** The tool is activated when `ctx.hasUI` (`index.ts:502`), and the browser opens as in the TUI. It uses `notify`, `setStatus` and `setWidget`, which RPC forwards. Its status text is themed with `ctx.ui.theme.fg(...)`, unverified with RPC's missing theme. | Render `setStatus` and `setWidget`; check the theme call in Phase 3. |
| `@juicesharp/rpiv-ask-user-question` (also installed) | **Degrades.** With `ctx.mode === "rpc"`, its RPC path asks one select or input dialog per question instead of its tabbed custom view. | Our dialogs are enough. A cpd-native questionnaire widget is a nice extra. |

### 2.2 JSON mode (`json.md`, `cli-integration.md` § Stream JSON events)

JSON mode produces the same events as RPC, plus a leading `session` header. It runs the prompts given at startup and
exits, so it has no steering, interrupt over the wire, or dialogs. It is useful for one-shot headless Workers, and as
the definition of the event schema we would share with RPC. Its failures do not set the exit status, so read the
events (`cli-integration.md` § Stream JSON events).

### 2.3 SDK (`sdk.md`)

- **The API.** `createAgentSession()` returns an in-process `AgentSession`: `prompt`, `steer`, `followUp`, `abort`,
  `waitForIdle`, and `subscribe(event => …)` with the full SDK events, which include cumulative `partial` snapshots
  (`sdk.md` § Prompting, § Subscribing to events; `json.md` § TypeScript types).
- **Sessions.** A `SessionManager` stores them, on disk or `inMemory()`. `AgentSessionRuntime` adds `newSession()`,
  `switchSession()`, `fork()` and `importFromJsonl()`; each one replaces the `AgentSession`, so subscriptions must be
  bound again (`sdk.md` § Session lifecycle, § Session storage).
- **Configuration.** Everything is injectable: model, settings, resource loader, tools (`sdk.md` § Configuring a session).
- **Extensions.** The CLI's built-in codemode, tool_search and MCP extensions are not loaded unless they are added
  (`sdk.md` § codemode-mcp). Guess: our file-based extensions load through `DefaultResourceLoader` as in the CLI.
- **Language.** TypeScript only, so the cockpit would gain a Node process and our own protocol between it and Rust.

### 2.4 `RpcClient` (`cli-integration.md` § Control Pi with RPC)

pi's own TypeScript client for RPC: it spawns pi, correlates commands, and offers `promptAndWait()`. It is useful for
option (b) or for tests, not for a Rust view.

## 3. A real protocol trace

**The command** (run from `/tmp/x`, which held three empty files):

    pi --mode rpc --no-session -ne -e /tmp/pi-rpc-demo/ext.ts \
       --model github-copilot/claude-haiku-4.5 --thinking off

- **Flags:** `--no-session` keeps the session in memory, so nothing goes under `~/.pi/agent/sessions/`. `-ne` drops
  every discovered extension, so cpd's adapter and the tmux state script stay out (`cli.md` § Resources). `-e` loads one
  scratch extension, [`pi-rpc-demo-ext.ts`](pi-rpc-demo-ext.ts), whose `/demo` command exercises the dialogs.
- **No live state touched:** no tmux server, no `~/.cpd`, no real session.
- **The driver** was [`pi-rpc-trace.py`](pi-rpc-trace.py):
  1. `get_state`;
  2. a prompt that uses one tool;
  3. a `steer` the moment the tool starts;
  4. `/demo`, answering its dialogs;
  5. a second prompt, interrupted with `abort` after three text deltas;
  6. `get_session_stats`;
  7. closing stdin.
- **The output.** [`trim-trace.py`](trim-trace.py) trimmed the raw log into [`pi-rpc-trace.jsonl`](pi-rpc-trace.jsonl):
  runs of deltas collapsed, long strings cut, the system prompt replaced by its section names, and the model's
  `baseUrl` and `headers` redacted. No credential appears in the protocol: pi reads them from its own `auth.json`.
- **Cost:** 4 model calls, 13.9k tokens, $0.017 (`get_session_stats`).

**Trimmed trace** (`>` is our stdin, `<` is pi's stdout; times in seconds):

```text
0.001 > {"id":"s1","type":"get_state"}
0.411 < {"id":"s1","type":"response","command":"get_state","success":true,"data":{"model":{"id":"claude-haiku-4.5","provider":"github-copilot",…},
         "thinkingLevel":"off","isStreaming":false,"isCompacting":false,"steeringMode":"one-at-a-time","followUpMode":"one-at-a-time",
         "sessionId":"01a13dc3-…","autoCompactionEnabled":true,"messageCount":0,"pendingMessageCount":0}}      # no sessionFile: --no-session
0.411 > {"id":"p1","type":"prompt","message":"List the files in /tmp/x with one bash call, then answer in one short line."}
0.417 < {"id":"p1","type":"response","command":"prompt","success":true,"data":{"disposition":"started"}}
0.417 < {"type":"agent_start"}
0.417 < {"type":"turn_start"}
0.418 < {"type":"message_start","message":{"role":"system","sections":"[preamble, tools, rules, docs, project_context, skills, cwd]"}}
0.418 < {"type":"message_end",  "message":{"role":"system",…}}
0.418 < {"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"List the files in /tmp/x …"}],…}}
0.418 < {"type":"message_end",  "message":{"role":"user",…}}
1.199 < {"type":"message_start","message":{"role":"assistant","content":[],"model":"claude-haiku-4.5","stopReason":"pending"}}
1.252 < {"type":"message_update","assistantMessageEvent":{"type":"toolcall_start","contentIndex":0,"id":"toolu_01Ui…","toolName":"bash"}}
1.438 < {"type":"message_update","assistantMessageEvent":{"type":"toolcall_delta","contentIndex":0,"delta":"{\"command\": \"ls -la /"}}
        … 2 more toolcall_delta ("tmp/x", "\"}")
1.440 < {"type":"message_update","assistantMessageEvent":{"type":"toolcall_end","contentIndex":0,"toolCall":{"type":"toolCall","id":"toolu_01Ui…","name":"bash","arguments":{"command":"ls -la /tmp/x"}}}}
1.441 < {"type":"message_end","message":{"role":"assistant","content":[{"type":"toolCall",…}],"stopReason":"toolUse","usage":{"input":2935,"output":58,…}}}
1.442 < {"type":"tool_execution_start","toolCallId":"toolu_01Ui…","toolName":"bash","args":{"command":"ls -la /tmp/x"}}
1.442 > {"id":"st1","type":"steer","message":"Also say how many files there are."}
1.446 < {"type":"tool_execution_update","toolCallId":"toolu_01Ui…","toolName":"bash","args":{…},"partialResult":{"content":[]}}
1.447 < {"type":"queue_update","steering":["Also say how many files there are."],"followUp":[]}
1.447 < {"id":"st1","type":"response","command":"steer","success":true,"data":{"disposition":"queued"}}
1.457 < {"type":"tool_execution_end","toolCallId":"toolu_01Ui…","toolName":"bash","result":{"content":[{"type":"text","text":"total 764\n… alpha.txt\n… beta.md\n… gamma.rs\n"}],…},"isError":false,"durationMs":…}
1.457 < {"type":"message_start","message":{"role":"toolResult","toolCallId":"toolu_01Ui…",…}}   + message_end
1.458 < {"type":"turn_end","message":…,"toolResults":[…1]}
1.460 < {"type":"turn_start"}
1.460 < {"type":"queue_update","steering":[],"followUp":[]}                                          # steer delivered here
1.460 < {"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"Also say how many files there are."}],…}}  + message_end
2.019 < {"type":"message_start","message":{"role":"assistant","content":[],…,"stopReason":"pending"}}
2.074 < {"type":"message_update","assistantMessageEvent":{"type":"text_start","contentIndex":0}}
2.074 < {"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"/"}}
        … 7 more text_delta
2.215 < {"type":"message_update","assistantMessageEvent":{"type":"text_end","contentIndex":0,"content":"/tmp/x contains 3 files: alpha.txt, beta.md, and gamma.rs."}}
2.216 < {"type":"message_end","message":{"role":"assistant",…,"stopReason":"stop","usage":{"input":3167,"output":26,…}}}
2.216 < {"type":"turn_end",…}
2.217 < {"type":"agent_end","messages":[…6],"willRetry":false}
2.217 < {"type":"agent_settled","aborted":false}
2.217 > {"id":"d1","type":"prompt","message":"/demo"}                                             # extension command
2.218 < {"type":"extension_ui_request","id":"a168d317-…","method":"setStatus","statusKey":"demo","statusText":"demo running"}
2.219 < {"type":"extension_ui_request","id":"6206f623-…","method":"confirm","title":"Proceed?","message":"Demo confirm"}
2.219 > {"type":"extension_ui_response","id":"6206f623-…","confirmed":true}
2.219 < {"type":"extension_ui_request","id":"fdcaa2ba-…","method":"select","title":"Pick one","options":["approve","reject"]}
2.219 > {"type":"extension_ui_response","id":"fdcaa2ba-…","value":"approve"}
2.219 < {"type":"extension_ui_request","id":"7bebf853-…","method":"notify","message":"mode=rpc hasUI=true confirmed=true picked=approve","notifyType":"info"}
2.219 < {"type":"extension_ui_request","id":"8b399abb-…","method":"setStatus","statusKey":"demo"}   # no statusText: clear
2.219 < {"id":"d1","type":"response","command":"prompt","success":true,"data":{"disposition":"handled"}}   # no run started
3.219 > {"id":"p2","type":"prompt","message":"Write a 400-word poem about terminals."}
3.222 < {"id":"p2","type":"response","command":"prompt","success":true,"data":{"disposition":"started"}}
        … agent_start, turn_start, user message, assistant toolcall_start "write", 305 toolcall_delta (7.9 s), toolcall_end,
          tool_execution_start/end (write /tmp/x/terminal_poem.txt), turn_end, turn_start
12.34 < {"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"Done"}}
        … 3 more text_delta
12.546 > {"id":"a1","type":"abort"}
12.546 < … 6 more text_delta already in flight
12.550 < {"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Done. A 400-word poem … to `/tmp/x/terminal_poem."}],
          "stopReason":"aborted","errorMessage":"This operation was aborted",…}}
12.550 < {"type":"turn_end",…}
12.550 < {"type":"agent_end","messages":[…4],"willRetry":false}
12.550 < {"type":"agent_settled","aborted":true}
12.550 < {"id":"a1","type":"response","command":"abort","success":true}                         # after idle, as documented
12.550 > {"id":"s2","type":"get_session_stats"}
12.552 < {"id":"s2",…,"data":{"userMessages":3,"assistantMessages":4,"toolCalls":2,"toolResults":2,"tokens":{"input":13201,"output":747,…},
          "cost":0.016936,"contextUsage":{"tokens":3900,"contextWindow":200000,"percent":1.95}}}
12.567   pi exited 0 on stdin close; stderr empty
```

**What the trace adds to the docs:**

- **A `role:"system"` message comes first in each run.** It is a `message_start`/`message_end` pair with `sections`
  and `toolsAdded` (about 10 kB), before the user message. `message-types.md` § SystemMessage documents the type; the
  sample in `json.md` § Event sequence does not show it. The view must hide it.
- **The steer was delivered after the tool result,** as a user message at the start of the next turn.
  `queue_update` showed it queued, then cleared (`rpc-commands.md` § steer).
- **Abort is not instant.** Deltas already in flight still arrive. The aborted message ends with `stopReason:"aborted"`.
  `agent_settled {aborted:true}` comes before the `abort` response.
- **Tool arguments stream as deltas too,** so a large `write` streamed 305 `toolcall_delta` records over 7.9 s. The view must
  render a running tool call without re-parsing its partial JSON on every delta. Show the tool name and a byte count
  until `toolcall_end`.
- **`get_state` has no `sessionFile` under `--no-session`.**
- **Extension dialogs work over RPC,** and `/demo` printed `mode=rpc hasUI=true`, as `rpc-extension-ui.md` §
  Limitations says.

## 4. Feature inventory: pi's TUI against our chat view

**Ratings:**
- **must:** the MVP or the Phase-3 cut-over needs it;
- **nice:** parity, later;
- **skip:** not for the cockpit.

The "Over RPC" column says what the protocol gives us; the rest we build.

### Display

| Feature | pi's TUI (source) | Over RPC | Rating | Our build (ratatui) |
| --- | --- | --- | --- | --- |
| Transcript of user and assistant text | `usage.md` § Follow Pi's work | `get_messages` on attach, then events (`json.md` § Message events) | must | `List`/`Paragraph` with `Scrollbar`, following the bottom |
| Markdown (headings, lists, emphasis, links) | `tui.md` § component model (`Markdown`) | raw Markdown text | must | Markdown to `Text`. Guess: the `tui-markdown` crate (0.3.11), its ratatui-0.30 compatibility unverified; else `pulldown-cmark` and our own spans |
| Code blocks, syntax highlighting | `settings.md` (`markdown.codeBlockIndent`) | raw fenced blocks | blocks must, highlighting nice | Block styling first; `syntect` later (guess) |
| Mermaid diagrams | `settings.md` (`markdown.mermaid`) | raw text | skip | Show the source |
| Tool calls and results, folded or unfolded | `usage.md` § Follow Pi's work (Ctrl+O) | `toolcall_*`, `tool_execution_*` (`json.md`) | must | One summary line per call (name, main argument, ✓/✗, duration), expanded on a key |
| Extension tool renderers (`renderCall`/`renderResult`) | `extensions.md` § Tool rendering | **not available**: TUI-only | skip; cpd's own tools get native widgets (nice) | Generic rendering, plus a native card for `cpd_*` tools |
| Edit diffs | edit tool result `details.diff` (`dist/core/tools/edit.d.ts`) | in `tool_execution_end.result` | nice | Colour `+`/`-` lines from the palette |
| Images | `tui.md` (`Image`); kitty graphics under tmux is an open point (vision) | `ImageContent` base64 (`message-types.md`) | skip | `[image]` placeholder |
| Thinking blocks | `usage.md` (Ctrl+T); this User hides them (`hideThinkingBlock`) | `thinking_*` deltas | nice | Collapsed `thinking…` line |
| Streaming deltas | live in the TUI | `message_update` deltas (`json.md` § Reconstruct streaming messages) | must | Grow the last block; replace it at `*_end` and `message_end` |
| Working indicator, retry notices | TUI working line | `agent_start`/`agent_settled`, `auto_retry_*` | must (working), nice (retry) | A status row |
| Custom messages (cpd plan decisions) | rendered by the TUI | `CustomMessage` with `display` (`message-types.md` § CustomMessage) | must | A cpd-native card |
| Compaction | `sessions.md` § Manage conversation context | `compaction_start`/`_end`, `CompactionSummaryMessage` | must (divider and status), nice (show the summary) | A separator plus a folded summary |
| Branches (`/tree`) | `sessions.md` § Choose how to branch | `get_tree` read-only; **no navigate** | skip | — |
| Footer: model, thinking, context %, cost | `usage.md` § Follow Pi's work | `get_state`, `get_session_stats` | nice | One `Line` with right alignment |

### Input

| Feature | pi's TUI (source) | Over RPC | Rating | Our build |
| --- | --- | --- | --- | --- |
| Multi-line editor (Shift+Enter, Ctrl+J) | `keybindings.md` § Input and selection | — | must | Guess: the `tui-textarea` crate family; its ratatui-0.30 support unverified (`tui-textarea` 0.7.0, a ratatui-core-based `tui-textarea-2` 0.13.2) |
| Paste (bracketed) | `keybindings.md` § Application | — | must | crossterm paste events |
| Image paste | `keybindings.md` (`app.clipboard.pasteImage`) | `prompt.images` | skip | — |
| History (↑ for previous prompts) | editor history (guess: TUI-local) | — | nice | Prompts taken from `get_messages` |
| External editor (Ctrl+G) | `keybindings.md` § Application | — | nice | `$EDITOR` in a tmux popup, like the legend |
| Slash commands with completion | `usage.md` § Change the model or settings; `slash-commands.md` | `get_commands` (extension, prompt and skill commands; **not** built-ins) | must for `/compact`, `/name`, `/model`; nice for the menu | Map the built-ins we keep to RPC commands; complete from `get_commands` |
| `@` file mentions, Tab path completion | `usage.md` § Enter a prompt | none: RPC rejects `@file` CLI arguments (`rpc.md` § Start RPC mode); prompt text is passed as typed | nice | Our own file finder (guess: `ignore` and a fuzzy matcher) |
| `!cmd` / `!!cmd` | `usage.md` § Run a terminal command | `bash {command, excludeFromContext}` (`rpc-commands.md` § bash) | nice | Parse the prefix, stream `bash_execution_update` |
| Keybindings: Enter, Esc, Alt+Enter, Alt+Up | `usage.md` § Change direction; `keybindings.md` | `prompt`, `steer`, `follow_up`, `abort`, `clear_queue` | must (Enter, Esc), nice (Alt+Enter, Alt+Up) | Our key map. Note that inside tmux some Alt keys need `extended-keys` (guess) |
| Transcript scroll and search | `keybindings.md` § Fullscreen | — | scroll must, search nice | `Scrollbar` and our own search |

### Control

| Feature | pi's TUI (source) | Over RPC | Rating |
| --- | --- | --- | --- |
| Interrupt (Esc) | `usage.md` § Change direction | `clear_queue` then `abort` (`rpc-commands.md` § clear_queue) | must |
| Steer, follow-up, queue display | `usage.md` § Change direction | `steer`, `follow_up`, `queue_update` | must (steer, queue), nice (follow-up) |
| Model switch | `/model`, Ctrl+L, Ctrl+P (`keybindings.md` § Models and Thinking) | `get_available_models`, `set_model`, `cycle_model` | nice |
| Thinking switch | `/thinking`, Shift+Tab | `set_thinking_level`, `cycle_thinking_level` | nice |
| Resume a session | `--session`, `/resume` | `--session` at spawn, `switch_session` | must |
| New, fork, clone | `/new`, `/fork`, `/clone` (`sessions.md`) | `new_session`, `fork`, `clone`, or `--fork` at spawn | nice; the cockpit's `n` keeps using `--fork` |
| Name | `/name` | `set_session_name`, `session_info_changed` event | must: the cockpit labels sessions by name |
| Session picker, delete | `/resume` picker (`keybindings.md` § Sessions) | none | skip: the Context column is the picker |
| Compact | `/compact` | `compact`, `set_auto_compaction` | must |
| Extension dialogs: select, confirm, input, editor | in-TUI dialogs | `extension_ui_request`/`_response` (`rpc-extension-ui.md`) | must: ask-user-question uses select and input |
| Notifications | in-TUI | `notify` | must: cpd's adapter only notifies |
| Status line, widgets | footer and editor widgets | `setStatus`, `setWidget` (string lines) | nice: plannotator uses both |
| Title, editor prefill | terminal title, `set_editor_text` | same | skip (title), nice (prefill) |
| Custom extension components | `ctx.ui.custom()` | **not available** | skip; cpd-native widgets instead |
| Settings, login, theme, export, share | `/settings`, `/login`, `/export`, `/share` | `export_html` only; theme switching fails (`rpc-extension-ui.md` § Limitations) | skip: use pi's CLI |

## 5. Architecture options

### (a) A Rust/ratatui chat view in `cpd-tui` driving `pi --mode rpc`, one child per Lead session — recommended

- **Shape:**
  - an `rpc` module: spawn, LF framing, serde-typed records, id correlation;
  - a transcript model, folding events into messages;
  - a `chat` view: transcript `List`, input editor, status row, dialog overlays drawn like `src/prompt.rs`.
- **Process ownership.** A per-socket `cpd-tui host` process owns one `pi --mode rpc --session <file>` per Lead. It runs
  in a hidden window of the cockpit's tmux server and talks to views over a Unix socket.
  - **Reconnect:** a view rebuilds the transcript with `get_messages` and `get_entries {since}`
    (`rpc-commands.md` § get_entries).
  - **Session switching:** the view re-points at another session id, with no `swap-pane`.
  - **"Needs you":** the host publishes the dot from `agent_settled` and the `ask_*` and plan-review tool events,
    replacing `tmux-agent-state.ts`, to a state file the Context view reads.
- **Gains:**
  - one visual language and palette (`src/palette.rs`), which the `research-components` Job can extend;
  - native cards for cpd custom messages, `cpd_plan_review`, Job references and questions;
  - instant switching;
  - the same `rpc` module drives headless Workers later, and the read-only Worker view can show live streams instead of
    re-reading files (guess).
- **Costs:**
  - re-implementing pi's editor, Markdown and tool rendering, its key handling, and its polish (completion, search);
  - the host process;
  - tracking pi's protocol across versions.
- **Risks:** see §1 (the adapter guard, process ownership, missing TUI-only features, drift). Also: a large paste or a
  300-delta tool call must not make ratatui redraw per record; batch events per frame, as the 250 ms loop already does.
- **Effort:** about 11 Jobs, 40–55 Worker-hours (§6). MVP after 5 Jobs.

### (b) A Node sidecar using pi's SDK

- **Shape:** a TypeScript process holds every Lead's `AgentSession` through `AgentSessionRuntime` (`sdk.md` § Session
  lifecycle) and exposes them to the Rust view over our own socket protocol.
- **Gains over (a):**
  - one process for all sessions, which also solves ownership;
  - full API access, such as cumulative `partial` snapshots and direct settings and resource control;
  - no stdout framing pitfalls.
- **Costs:**
  - we define and version our own protocol, which duplicates RPC (DRY);
  - a second language and runtime in the cockpit, against the "one Rust binary" goal (vision decision 9);
  - the SDK does not load the CLI's built-in extensions by default (`sdk.md` § codemode-mcp);
  - the rendering is still ours to write in Rust.
- **Effort, guess:** (a) plus 2–3 Jobs for the sidecar and its protocol; the sidecar replaces only (a)'s host Job.
  **Not recommended:** RPC already is that protocol, maintained upstream (Convention over configuration).

### (c) Keep pi's TUI in tmux panes, as today

- **Gains:** zero cost; full parity always, including `/tree`, custom extension UI, images and themes; pi's upgrades
  arrive for free.
- **Costs:**
  - two visual languages;
  - no inline cpd widgets;
  - switching sessions by `swap-pane`;
  - state reaches the cockpit only through tmux options published by `tmux-agent-state.ts`;
  - plannotator and ask-user-question keep their own UIs.
- **Effort:** none. It stays the default until (a) reaches Phase 3.

### Weighing

The gains of (a) are real but mostly visual and ergonomic. The cost is concentrated in work pi already does well:
the editor, Markdown and tool rendering, which is about half the estimate. So build (a) in phases, and keep (c) as the
default and fallback until the must-have rows of §4 hold. (a)'s view stays usable read-only even before input parity,
as the Worker view's transcript.

## 6. Phased plan (proposed Jobs)

Sizes are Worker-hours, guesses, at this Workflow's pace. S is 2–3 h, M is 4–5 h, L is 6–8 h. "cpd" marks a Job outside
`tui/`, which needs a Workflow other than `cpd-ui`.

**Phase 0: prerequisites**

| Job | Goal | Size |
| --- | --- | --- |
| `pi-adapter-rpc` (cpd) | Make the cpd adapter run under `--mode rpc`: find why `pi/extension.ts:176` guards on `ctx.mode !== "tui"`, then guard on `ctx.hasUI` instead; tests for both modes. | S |
| `rpc-client` | `tui/src/rpc.rs`: spawn `pi --mode rpc`, LF-only framing, serde types for the commands, responses, events and extension UI records we use (unknown events kept raw), id correlation, stderr captured, exit handled. Tests against recorded fixtures such as `pi-rpc-trace.jsonl`. | M |

**Phase 1: MVP (transcript, input, send, stream, interrupt), opt-in beside today's pane**

| Job | Goal | Size |
| --- | --- | --- |
| `chat-transcript` | A transcript model folded from `get_messages` and events, with the delta rules of `json.md`. A `chat` view: user and assistant text with basic Markdown, folded tool calls, hidden system messages, a working/idle status, scroll with `Scrollbar`. | L |
| `chat-input` | A multi-line editor (choose the crate after checking ratatui-0.30 support), paste, Enter to send or steer while streaming, Esc to `clear_queue` + `abort` with the text restored, the queue shown. | M |
| `chat-dialogs` | Extension UI: select, confirm, input and editor as overlays answering `extension_ui_response` (cancel on Esc, honour `timeout`); `notify` as a toast; `setStatus` in the status row. | M |

The MVP ends here: `cpd-tui chat --session <file>` drives one Lead in a scratch tmux server. Five Jobs, about
18–24 Worker-hours.

**Phase 2: in the cockpit**

| Job | Goal | Size |
| --- | --- | --- |
| `chat-host` | A `cpd-tui host` that owns one pi per Lead session (only owner of each session file), multiplexes views over a Unix socket, rebuilds on reconnect with `get_entries {since}`, and publishes `@agent_state`, "needs you" and identity for the Context view in place of `tmux-agent-state.ts`. | L |
| `chat-switch` | The centre chat pane becomes the `chat` view. Switching (tabs, tree, Enter) re-points it at the session, with no `swap-pane`. `n` (fork) and `x` (close) go through the host. | M |

**Phase 3: parity and cut-over**

| Job | Goal | Size |
| --- | --- | --- |
| `chat-commands` | Slash commands: `/compact`, `/name`, `/model`, `/thinking` mapped to RPC; completion from `get_commands`; a model picker (`Table`) and a thinking cycle; a footer with context % and cost. | M |
| `chat-render-plus` | Syntax highlighting, edit diffs from `details.diff`, a compaction divider and summary, retry notices, collapsed thinking, `setWidget` lines. | M |
| `chat-cpd-widgets` | cpd-native cards inline: plan-decision custom messages, `cpd_plan_review` with approve/reject, Job references from the Plan view. With `research-components`. | M |
| `chat-cutover` | Make the `chat` view the default for Leads, keep `pi` in a tmux pane as a fallback switch, and update `tui/README.md`. | S |

**Later, separate:** headless Workers on the same `rpc` module (a cpd-rust concern, vision decision 5); `@` file
completion, `!cmd`, history, transcript search (nice).

## Open questions

- **Why does `pi/extension.ts` guard on `ctx.mode !== "tui"`?** It came in with `b437a78` ("pi-adapter: queue delivery, herdr blocked state, cpd_plan_review tool, unit tests"); I found no stated reason.
  The guard must be understood before it changes (Phase 0).
- **Do `tui-textarea` and `tui-markdown` support ratatui 0.30?** Unverified: `cargo info` did not list their ratatui
  requirement. If not, the editor Job grows, or we fork, which is against the axioms.
- **Does plannotator's themed `setStatus` work over RPC?** `ctx.ui.theme.fg(...)` while `getTheme()` returns
  `undefined` (`rpc-extension-ui.md` § Limitations) is unverified. I did not load plannotator in the trace, to keep the
  run scratch-only.
- **How do kitty keyboard reporting and Alt+Enter behave inside tmux 3.4** for our own key handling? This is the
  vision's open point; untested here.
- **Does the host process need a lock** so a resumed tmux pi and the host can never open the same session file at once?
  pi itself does not say it locks (guess: no lock).
