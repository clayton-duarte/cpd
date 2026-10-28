# ratatui chat, parity audit (core): prompt, tools, MCP, compaction, models

Evidence file for Question 2 of Plan `ratatui-chat-research` (decision pro-d166, User, 2026-10-28 EDT), first half. The other half is
`docs/research/ratatui-chat-parity-runtime.md`. A synthesis Job reads this file. Research only: no product code is changed.
Written 2026-10-28 15:00-16:00 EDT by Worker `parity-core-a1`.

## How to read this file

- **Labels.** Every claim carries one of: **read** (a file and line I opened, `path:line`), **ran** (a command I ran, output quoted)
  or **unverified** (inferred, with why and how to check).
- **Path prefixes.** `pi:` is the `earendil-works/pi` repo at tag `v1.4.0` (commit `4a080252`, **ran** `git log -1`, cloned into a
  temporary directory, deleted at the end); `pi:` paths are relative to it, for example `pi:packages/durable/src/harness/types.ts`.
  `cpd:` is this repo at `origin/main` `d943c49c` (**ran** `git rev-parse --short origin/main` after `git fetch origin`).
  A path with no prefix is a cpd path.
- **Versions.** cpd runs pi **1.3.0** (**ran** `grep '"version"' node_modules/@earendil-works/pi-coding-agent/package.json` gives
  `1.3.0`); everything below about pi and pi-durable was read at **1.4.0**. Differences I met are in "Version differences".
- **"pi-durable"** is `@earendil-works/pi-durable@1.4.0`; its README is `pi:packages/durable/README.md`. I did not run pi, a model or
  the adapter (out of scope), so nothing here is a run of pi-durable: where a claim is about how it behaves, it is **read** from source
  or **unverified**.
- **Sizes.** Every size is lines of TypeScript (or Rust) with its basis: a comparable file measured with `wc -l` (all measurements are
  in "Commands run"). A size is my estimate, not a measurement of code that does not exist.

## Verdict in six lines

1. **Prompt (a, b, c):** pi-durable loads nothing (no skills, no `AGENTS.md`, no settings; **read** only a test helper mentions them).
   But its `section()` is the same idea as pi's `systemPromptOptions.sections`, and `pi-coding-agent` **exports** the loaders
   (`loadProjectContextFiles`, `loadSkills`, `formatSkillsForPrompt`), so the cheapest close is a ~100-line extension that calls them.
2. **MCP (d):** pi-durable has **no MCP client**. `@earendil-works/pi-mcp` (the protocol client) **is** usable beside it (one
   dependency, `cross-spawn`; imports no pi package). pi's MCP **extension** is not: it is written against pi-coding-agent's
   `ExtensionAPI`. Cheapest: a durable extension of ~450 lines over `pi-mcp`, tools registered `deferred`/`callers: ["tools"]` for
   cpd's exposures. `pi mcp login` can stay the sign-in (shared `mcp-auth.json`, unverified).
3. **Compaction (e):** pi-durable **has it built in** (`CompactionTask`, auto thresholds, `compact()`, overflow retry, a committed
   `pi.compaction` entry, a `beforeCompact` hook), with the same summary format as pi's. Missing: cpd's **ratio** reserve (a
   ~30-line `modelOverrides` provider) and a durable port of 1.4.0's "summary uses the model's output limit" fix.
4. **Models (f):** pi-durable takes a pi-ai `Models`; pi-coding-agent's `ModelRuntime` **implements** that interface and reads
   `auth.json` and `models.json` (so `bifrost` and `github-copilot` keep working). pi-ai itself does **not** read `auth.json`.
   Retier becomes `configure({ model, thinkingLevel })`.
5. **Tools and guards (g):** pi-durable's `CodingTools` has `read`, `write`, `edit`, `bash` only: **no `grep`, `find`, `ls`**
   (pi's defaults are the same four; those three are opt-in in pi). The seven tool_call guards are pure functions and map to
   `beforeTool` (`block`), with two gaps: the hook gets no `cwd`/transcript argument (askguard's `unseen`, mainguard's `cwd`).
6. **Size (this half):** ~800 new TypeScript lines (range 650-1000) in 6 modules and 4 PRs, ~100 Rust lines, and **~650 lines of
   existing guard logic reused unchanged**. Basis in "Size estimate".

## Feature table

Columns: feature | cpd today | pi-coding-agent (pi) | pi-durable | gap | how to close (kind, size, basis).
Sizes are TypeScript lines unless stated; the bases are listed under "Size estimate".

| Feature | cpd today (file:line) | pi-coding-agent provides (pi file:line) | pi-durable provides | Gap | How to close |
|---|---|---|---|---|---|
| **(a) Skills** `cpd-manager` (143 lines), `cpd-lead` (409 lines); progressive loading; `--no-skills` for Workers | `pi/package.json` `pi.skills: ["./skills"]` (**read**); `pi/skills/{cpd-lead,cpd-manager}/SKILL.md` (**ran** `wc -l`); Worker: `core/src/host/pi/launch.rs:168` `--no-skills`; distiller `launch.rs:198` | `loadSkills` `pi:packages/coding-agent/src/core/skills.ts:414`; `formatSkillsForPrompt` `skills.ts:358` lists name, description, file path in `<available_skills>` and tells the model to `read` the file when needed (progressive); `--no-skills` `pi:packages/coding-agent/src/cli/args.ts:210`; both exported from `src/index.ts:325-329` (**read**) | **nothing**: `git grep -in skill -- packages/durable/src packages/durable/README.md` finds only `src/testing/env-conformance.ts:306-323` (a file-watch test of `skills/a/b/SKILL.md`) (**ran**) | No skill list in the prompt; no `/skill:` command (a UI matter, `parity-runtime`). The `read` tool exists (`CodingTools`), so progressive loading only needs the list section | **Reuse (dependency on `@earendil-works/pi-coding-agent`)**: a `section("skills", ...)` calling `loadSkills` + `formatSkillsForPrompt`; Workers: do not install it. **~30 lines**; basis `pi/lib/policy.ts` (25 lines, one pure formatter) + the `section()` calls at `pi:packages/durable/test/examples/26-coding-agent.ts:32-33`. Port instead of dependency: ~150 lines, basis `skills.ts` 514 lines minus the CLI/diagnostic parts (unverified split) |
| **(b) `AGENTS.md` and context files** (`~/.pi/agent/AGENTS.md`, project, ancestors) | Not implemented by cpd: pi loads it, hs-d611 relies on it (`docs/research/pi-durable.md` item 6; distiller comment `launch.rs:193-196`) | `loadProjectContextFiles` `pi:packages/coding-agent/src/core/resource-loader.ts:232-274`: global file in the agent dir, then each ancestor of `cwd`, names `AGENTS.override.md`, `AGENTS.md`, `AGENTS.MD`, `CLAUDE.md`, `CLAUDE.MD` (`:185`), with a worktree-shadow rule (`:225-230`); exported `src/index.ts:248`; `--no-context-files` `args.ts:216` | **nothing**: `git grep -n AGENTS -- packages/durable/src packages/durable/README.md` finds only `src/testing/env-conformance.ts:276-289` (**ran**); **re-verified: the earlier doc's claim holds** | The axioms never reach a durable agent | **Reuse**: a `section("context-files", ...)` over `loadProjectContextFiles({cwd, agentDir})`. **~20 lines**; basis the function itself is 42 lines (**ran** `awk` count). Note it reads the host file system, not the durable `env`: fine for a local runner |
| **(c) Policy and time-zone sections** | `pi/lib/policy.ts:126` `policySection` (pure), `POLICY_SECTION` `:118`; `pi/lib/time.ts:190` `zoneLine`, `:195` `TIME_ZONE_SECTION`; wiring `pi/lib/agent-state.ts:30-46`: two `before_agent_start` handlers write `event.systemPromptOptions.sections[key]` (missing `sections` is the adapter error `sections_missing`, `:39`) | `sections?: Record<string,string>` in the system-prompt input `pi:packages/coding-agent/src/core/system-prompt.ts:30`; extension handlers get it mutable `src/core/extensions/types.ts:936`; sections are diffed against the transcript `system-prompt.ts:213` | **has it**: `section(key, render, {tag})` `pi:packages/durable/src/harness/define.ts:20`, `render` may be async (`types.ts:358`), rendered in extension order before each request; "only what changed is sent again" and **a section that changes every time, "such as the current time", defeats the prompt cache** (README "System Prompt") | Different API (a `sections` array on `defineExtension`, not an event handler); async `render` can call `cpd policy show`. `zoneLine` embeds the current minute (`time.ts:192` uses `proseTime(now)`), the same cache cost as in pi (unverified: whether pi's diff at `system-prompt.ts:213` re-sends on every change) | **Port, tiny**: the two pure functions stay; two `section()` registrations. **~40 lines**; basis `pi/lib/agent-state.ts:30-46` (17 lines) plus an async `cpd policy show` call like `pi/lib/cpd.ts` (87 lines, reused). Optional cache fix: render the line at day granularity (a behaviour change, 0 lines) |
| **(d) MCP** (own subsection below) | `pi/mcp-extension.ts:4` + `pi/lib/mcp.ts:201` `mcpMount` (pi's extension, no file config); `launch.rs:91` `--no-mcp`; `pi/lib/mcp.ts:206` `attachServer` -> `pi.registerMcpServer`; `pi/lib/handle.ts:107` handles `mcp-attached`; policy `core/src/policy/machine.rs` (290 lines), attach CLI `core/src/mcp.rs` (75 lines) | `createMcpExtension` `pi:packages/coding-agent/src/extensions/mcp/index.ts:279`; `registerMcpServer` `src/core/extensions/loader.ts:474`; exposures `codemode|deferred|direct|hidden` `src/core/mcp-servers.ts:52`; `mcp__<server>__<tool>` `index.ts:9-13` doc, `tools.ts:78` naming; `/mcp` manager `ui.ts`; `pi mcp login` `cli.ts` | **nothing** for MCP itself: `git grep -il mcp -- packages/durable/src packages/durable/package.json` finds five files, all comments or prefix examples (`codemode.ts:218`, `describe.ts:13`, `agent.ts:98`, `types.ts:321`, `tool-search.ts:14`) (**ran**). It has the **hooks to host MCP tools**: `deferred: true` `types.ts:329`, `callers: ["tools"]` `types.ts:324`, `tool_search` `pi:packages/durable/src/tool-search/tool-search.ts`, `createCodemodeExtension` with `namespaces` | No client, no config, no OAuth, no `/mcp`. pi's extension cannot be loaded (see (d) below) | **Build** an extension over `@earendil-works/pi-mcp`: **~450 lines**, basis `runtime.ts` 454 + `config.ts` 243 + `tools.ts` 391 (stdio, file config and TUI parts dropped, about half kept). Details in subsection (d) |
| **(e) Compaction** (own subsection below) | `pi/lib/compaction-ratio.ts:67-69` wraps `SettingsManager.prototype.getCompactionReserveTokens` (`hookMethod`); registered `pi/lib/adapter.ts:37`; the fold drawing `pi/compact-tools/extensions/compact-tools-compaction.ts` (91 lines, replaces `updateDisplay` of pi's component, `:6-8`) | `/compact` `slash-commands.ts:40`, `AgentSession.compact` `agent-session.ts:2828`; thresholds `contextTokens > contextWindow - reserveTokens` (`docs/compaction.md:32`), defaults 16384 / 20000 (`settings-manager.ts` `DEFAULT_COMPACTION_TOKEN_SETTINGS`, `:56`), per-model `modelOverrides`; summary prompt `compaction/compaction.ts:507`; extension events `session_before_compact` `types.ts:786` | **has it, built in**: `CompactionTask` `pi:packages/durable/src/harness/compaction.ts:112`, thresholds `generation.ts:344`, `root.compact(instructions)` `types.ts:747`, `settings.compaction` + `modelOverrides` `types.ts:505-524`, `beforeCompact` hook `types.ts:890`, committed entry `pi.compaction` `entries.ts:34`, same summary format `compaction.ts:74-100` | No **ratio** reserve (cpd's rule); 1.4.0's "summary uses the model's output limit" fix is not in durable (`compaction.ts:148-151` still caps at `0.8 * reserveTokens`); the fold drawing is a UI matter (`parity-runtime`, widgets) | **Config + port**: `modelOverrides` provider for the ratio (**~30 lines**, basis `compaction-ratio.ts` 69 lines minus the prototype hook), `/compact` = one `compact()` call, optional `beforeCompact` for a custom summary. Details in subsection (e) |
| **(f) Models, thinking, auth, catalog** | `pi/lib/retier.ts:50-62` `ctx.modelRegistry.find` + `pi.setModel` + `pi.setThinkingLevel`; `pi/lib/models-list.ts` (177 lines, runs under pi's Node on pi's `ModelRuntime`); Tiers `docs/contract.md` §9 (`:4204`), `[model]` domain `:4022`; catalog read via pi's Node `ModelRuntime.create` + `getSupportedThinkingLevels` (§9, `:4208-4215`) | `ModelRuntime` `pi:packages/coding-agent/src/core/model-runtime.ts:174` (`implements Models`), reads `auth.json` (`config.ts:623`, `auth-storage.ts:64`) and `models.json` (`model-config.ts:158-198`); order of credential sources `docs/models.md:23`; `getSupportedThinkingLevels` `pi:packages/ai/src/models.ts:1260`; levels `off,minimal,low,medium,high,xhigh,max` `pi:packages/ai/src/providers/model-schema.ts:3-4` | **partly**: `HarnessOptions.models: Models` (`types.ts:616`); per-conversation `configure({ model, thinkingLevel })` `types.ts:729`, applies from the next request (README "Per-Conversation Agent"); **no `auth.json`**: pi-ai's `createModels({credentials})` takes an app-owned `CredentialStore` (default in-memory) `pi:packages/ai/src/models.ts:376-382`, `auth/credential-store.ts:9` | Keys: durable needs a `Models`. `bifrost` is a `models.json` custom provider (unverified in this repo: `git grep -n bifrost -- . ':!docs/research'` finds only tests, `cli/tests/engine/watch_inbox_models.rs`; the provider config lives in the User's `~/.pi/agent/models.json`), built by pi-coding-agent, not pi-ai | **Reuse** `ModelRuntime.create({allowModelNetwork:false})` as the `models` (type-compatible by `implements Models`, unverified at run time): **~20 lines**. Retier: **~40 lines** (basis `retier.ts` 73 lines, minus the idle wait the harness makes unnecessary, unverified). No-dependency alternative: a file-backed `CredentialStore` over `auth.json` ~70 lines (basis `credential-store.ts` 67 lines) **plus** a models.json provider builder (basis `model-config.ts` 219 lines): rejected |
| **(g) Tools** | Tool set: pi default `read, bash, edit, write` (`pi:packages/coding-agent/src/core/settings-manager.ts:85`); Workers: `--exclude-tools` `launch.rs:14-15,168`; guards `pi/lib/adapter.ts:70-103` | built-ins `src/core/tools/{read,write,edit,bash,grep,find,ls}.ts` (`wc -l`: 224, 97, 220, 442, 325, 318, 175); `tool_search` `src/extensions/tool-search/tool.ts` (244; inactive by default `index.ts:5-7`); `codemode` `src/extensions/codemode/` | `CodingTools` = `read, write, edit, bash` `pi:packages/durable/src/tools/index.ts:27-37`; **no `grep`/`find`/`ls`**: `git grep -n 'grep\|"find"\|"ls"' -- packages/durable/src packages/durable/package.json` printed nothing (**ran**); `createToolSearchExtension` (`tool-search/tool-search.ts`, 92 lines); `createCodemodeExtension` (`codemode/codemode.ts`, 1044 lines) | The coding-agent's default tool set is also four tools, so **no gap for the defaults**; `grep/find/ls` only matter if cpd enabled them (**ran** `grep -c '"--tools"' core/src/host/pi/launch.rs` -> 0: it passes no `--tools`, so pi's four defaults apply (`agent-session.ts:3712`)). Write/edit differences: `edit-diff.ts` 500 lines vs the agent's `edit.ts` 220: unverified parity | Install `CodingTools`, `createToolSearchExtension`, `createCodemodeExtension`: **0 lines of port**, three `registry.install` lines. Tool rendering is UI (`parity-runtime`) |
| **(g2) Guard: `mainguard`** (primary checkout writes, `git` moves) | `pi/lib/mainguard.ts:225` `primaryCheckoutMove(command, cwd, home)`, `:246` `primaryCheckoutWrite(tool, input, cwd, home)` (253 lines); `adapter.ts:74-85` | `tool_call` returns `{block, reason}`; `ctx.cwd` | `hook(ToolTask, { beforeTool(call, api, ctx) => ({ block }) })` `types.ts:873-879`; first `block` wins; a throw blocks | `cwd` is not an argument of the hook: **unverified** how to read the conversation's `cwd` from `HookApi` (it is a `DocumentReader`, `types.ts:840`; the agent document `pi.agent` stores `cwd`, README "Per-Conversation Agent"); verify with a 20-line probe | **Port glue**: pure functions unchanged; **~15 lines** (basis `adapter.ts:74-85`, 12 lines) |
| **(g3) Guards: `secretguard`, `hostapprove`, `noblock`, `testguard`** (bash command checks) | `secretguard.ts:5` (23 lines), `hostapprove.ts:9` (60; also registers a tool `:28`), `noblock.ts:72` (84; Manager/Lead refused waits), `testguard.ts:77` (85; Worker unfiltered test runs); `adapter.ts:78-103` | same as above | same `beforeTool`; `call.name === "bash"`, `call.arguments.command` | none for the bash-command guards. `hostapprove`'s registered tool needs `defineTool` (`host.ts`/`hostapprove.ts:28`, ~30 lines, **unverified**: not read in full) | **Port glue** **~40 lines** for the four; basis `adapter.ts:94-102` (9 lines) x per-role selection |
| **(g4) Guard: `askguard`** (`ask_user_question` too long / unseen) | `pi/lib/askguard.ts:29` `tooLong`, `:86` `unseen(input, toolCallId, branch)` (104 lines); `adapter.ts:86-93` reads `ctx.sessionManager.getBranch()` | same | `beforeTool` works for the call; the **transcript** is read with `api` document readers (`HookApi extends DocumentReader`); `ask_user_question` itself does not exist in durable: the program defines it (a tool whose `execute` waits for the human) | `unseen` needs the entries before the call; **unverified** how to read them from `HookApi` (check `pi:packages/durable/src/harness/view.ts`) | **Port** **~40 lines** (basis `askguard.ts` 104 lines is pure; only the branch accessor changes). The tool and its dialog belong to `parity-runtime` |
| **(g5) Guards: `pnguard`, `loop-guard`** | `pnguard.ts:23` `pnVerdict` (40 lines, plannotator tool calls); `loop-guard.ts:48` is a `before_agent_start` counter (60 lines) | `before_agent_start`, `tool_call` | `pnguard`: a `beforeTool`; `loop-guard`: `GenerationTask` hook `beforeRequest` or `onYield` (`types.ts:852-865`), which can also `continue` a run | `loop-guard` also appends a session entry (`appendEntry`) and refreshes the pill: durable's own state documents (README "Your Own State") replace `appendEntry` | **Port** **~50 lines** (basis `loop-guard.ts` 60 lines) |
| **(h) Worker tool exclusion and mode flags** (`--exclude-tools`, `--approve`, `--no-mcp`) | `launch.rs:14-15,168-172` | CLI flags `args.ts` | no CLI; the program picks tools: `configure({ tools })` per conversation (README "Per-Conversation Agent") or installs no extension | none for tools; `--approve` is a pi-only prompt (nothing to approve in durable) | **0 lines** (do not install the Lead-only tools into a Worker conversation) |
| **(i) Thinking effort levels and the Tier letters O/S/H/G** | `docs/contract.md:4206-4210` (Tier -> level map `0 off ... 5 max`, `minimal` never used); `[model]` `:4022` | `--model <id>:<thinking>` parsed in `model-resolver.ts:77`; `getSupportedThinkingLevels` | `thinkingLevel?: ModelThinkingLevel` per conversation `types.ts:414,438,456`; the same pi-ai type, so the **same seven values** (**read** `model-schema.ts:3-4`) | The `provider/model:level` string form is a pi CLI syntax; durable takes `{model:{provider, modelId}, thinkingLevel}` | **Port** a ~10-line splitter in the runner; the Tier maths is Rust and runner-neutral |
| **(j) Model catalog and discovery** (E15) | `pi/lib/models-list.ts` (177 lines, **runs under pi's Node**, imports pi's dist through `ModelRuntime`) | `ModelRuntime.getAllModels/getAvailable/refresh` | pi-ai `Models` (what `ModelRuntime` implements) | none: the helper is not a pi extension, it already runs outside pi | **Nothing to replace** while pi is installed; a durable-only world would use `createModels()` and lose `models.json` custom providers |

## (d) MCP

### What cpd does today

- **Attach, not autoload** (contract §6.11, **read** `docs/contract.md:3521`): pi launches with `--no-mcp` (`launch.rs:91`, **read**), the
  adapter mounts pi's own MCP extension with an empty file config (`pi/lib/mcp.ts:201-203`, **read**), and `cpd mcp attach` emits an
  `mcp-attached` Event whose handler calls `pi.registerMcpServer(name, config)` (`pi/lib/mcp.ts:206-213`, `pi/lib/handle.ts:107`, **read**).
- **Machine policy** `machine.mcp.servers.<name>`: `url`, `description`, `exposure`, `toolExposure`, `oauth.{clientId,scope,callbackUrl,clientName}`,
  `headers` (**read** `docs/contract.md:4094`). **Correction to the Job's wording:** the exposures are
  `codemode`, `deferred`, `direct`, `hidden` (four, **read** `docs/contract.md:4094` and `pi:.../mcp-servers.ts:52`), and pi's default is
  `codemode`, not `direct`. HTTP only; no env, no command, no credential in cpd.
- **Sign-in:** `/mcp login <name>` inside pi, or `pi mcp login <name>` in a shell (**read** `docs/commands.md:5622`). Credentials are pi's.
- **Helpers outside the chat that already use pi's MCP client:** `pi/lib/slack-channel.ts:140` and `pi/lib/jira-transition.ts:138` build
  `new pi.McpServerConnection({... credentials: new pi.McpOAuthCredentialStore() ...})` after importing pi's `dist` files by path
  (`slack-channel.ts:117`, **read**). They are separate `node` processes, not extensions: **runner-neutral**, they need no change.
  `McpServerConnection` and `createDefaultTransport` are **not** exported from pi's package index (only `createMcpExtension` is,
  **ran** `grep -n 'createMcpExtension\|McpServerConnection' src/index.ts` -> line 413 only), which is why the helpers import by path.

### What pi-coding-agent does

- Built-in extension `createMcpExtension` (`pi:packages/coding-agent/src/extensions/mcp/index.ts:279`, 1347 lines, **read**): loads
  `mcp.json` (`config.ts:146`) and `pi.registerMcpServer()` servers, connects in the background, registers
  `mcp__<server>__<tool>` tools, adds an `mcp_servers` prompt section (`index.ts:156`), resource tools (`resources.ts`), `/mcp` UI (`ui.ts`),
  OAuth with credentials in `<agent-dir>/mcp-auth.json` (`oauth.ts:10`), and a CLI `pi mcp add|login` (`cli.ts`).
- Exposure semantics (**read** `mcp-servers.ts:41-52`): `codemode` (default; callable from codemode scripts only), `deferred` (not declared
  until `tool_search` loads it), `direct`, `hidden`; per-tool `toolExposure` overrides, exact name over patterns.

### What pi-durable has, and whether pi's MCP can run in it

- **pi-durable has no MCP capability** (**ran** `git grep -il mcp -- packages/durable/src packages/durable/package.json`: five files, all
  comments or prefix examples). It has the **sockets** an MCP host needs: `callers: ["tools"]` ("tools behind code mode, such as MCP tools",
  `types.ts:321`), `deferred: true` (`types.ts:329`) with `tool_search`, and code-mode `namespaces` with the example prefix `mcp__github__`
  (`describe.ts:13`).
- **`@earendil-works/pi-mcp` (the client) can be used as is.** **ran** `npm view @earendil-works/pi-mcp@1.4.0 dependencies peerDependencies`
  -> `{ 'cross-spawn': '7.0.6' }`, no peers. **ran** `git grep -n "pi-agent-core\|pi-coding-agent" -- packages/mcp/src` printed nothing. Its README
  says it "does not depend on the official MCP SDK or other pi packages" and shows wrapping a tool for an LLM (`toLlmContent`,
  `pi:packages/mcp/src/protocol/content.ts:112`) and OAuth (`./oauth`). It has `McpClient` (`client.ts:155`) and `StreamableHttpTransport`
  (`transports/streamable-http.ts:194`). cpd's servers are HTTP only, so stdio is not needed.
- **pi's MCP extension cannot install in pi-durable.** It is an `ExtensionFactory` over pi-coding-agent's `ExtensionAPI`
  (`index.ts` imports `ExtensionAPI`, `ExtensionContext`, `ToolDefinition` from `../../core/extensions/types.ts`, **read**); `tools.ts:14,23`
  imports `pi-agent-core` and `pi-tui` types and the interactive components (`tools.ts:35-36`); `oauth.ts:35-36` imports `getAgentDir` and
  `FileAuthStorageBackend` from the coding agent. The durable `defineExtension` API is different (`tools`, `sections`, `hooks`, `wraps`, `tasks`).
  **Verdict: it needs an adapter (a rewrite of the registration and tool-definition layer), it cannot be loaded as is.** The reusable parts
  are pure: `mcpNamespace`/exposure resolution (`mcp-servers.ts`, 346 lines, no pi imports, **read**: imports none) and the tool-name
  function (`tools.ts:78`).
- Other npm packages: `pi-mcp-adapter@7.0.0` and `pi-mcp-extension@1.11.7` are for the pi coding agent (**ran** `npm view pi-mcp-adapter@7.0.0
  peerDependencies` -> `@earendil-works/pi-tui: '*'`, pi-ai); I found none for pi-durable (**ran** `npm search --json "pi-durable mcp"`: no MCP package for durable among
  12 results). Not taken.

### Cheapest way to add it, and its size

A cpd extension `mcp` for pi-durable, taking `@earendil-works/pi-mcp` (axiom: convention over configuration):

| Piece | Lines | Basis |
|---|---|---|
| Connect each attached server with `McpClient` + `StreamableHttpTransport`, headers, timeouts, reconnect | ~120 | `runtime.ts` 454 lines (stdio/config/log dropped) |
| One `defineTool` per MCP tool, name `mcp__<server>__<tool>` (copy of the pure naming function), schema -> `Type.Unsafe`, `toLlmContent`; exposure -> `deferred: true` (deferred), `callers: ["tools"]` (codemode), `["model","tools"]` (direct), not registered (hidden) | ~150 | `tools.ts` 391 lines minus renderers, README example (15 lines) |
| `toolExposure` patterns, `description` into the `codemode` namespace, install via `createCodemodeExtension({namespaces})` and `createToolSearchExtension()` | ~60 | `mcp-servers.ts` 346 lines (the exposure functions are a part) |
| `mcp-attached` Event -> install the extension again under the same name (README "Reload": installing a same-named extension replaces it) | ~40 | `pi/lib/mcp.ts` + `handle.ts:107` (18 + ~15 lines) |
| OAuth: **keep `pi mcp login`**, which writes `<agent-dir>/mcp-auth.json` (`oauth.ts:10`); the durable extension reads it through the pi-mcp `AuthProvider` | ~80 | `oauth.ts` 558 lines (store + refresh only) |
| **Total** | **~450** (range 350-600) | |

Optional extras that cpd gets from pi today and would lose: the resource tools (`list_mcp_resources`, `read_mcp_resource`;
`resources.ts` 339 lines -> ~120 lines) and an in-chat `/mcp login` (a UI action, `parity-runtime`). Whether the credential file written by
`pi mcp login` can be read by a durable extension without importing pi-coding-agent's `McpOAuthCredentialStore` is **unverified**; it is not exported from
the index (`index.ts:413`), so it is a deep import of `dist/extensions/mcp/oauth.js` as `slack-channel.ts:117` does, or a re-implementation of
`mcp-auth.json` (~80 lines counted above). Verify with a probe that connects `atlassian` through the durable extension.

## (e) Compaction and context-window management

### What pi-coding-agent does (read)

- **Trigger:** `contextTokens > contextWindow - reserveTokens` (`pi:packages/coding-agent/docs/compaction.md:32`), checked before a prompt, between
  turns after tools, and as overflow recovery (`compaction.md:37`); manual `/compact [instructions]` (`slash-commands.ts:40`, `compaction.md:41`).
- **Settings:** `compaction.{enabled,reserveTokens=16384,keepRecentTokens=20000,modelOverrides}` (`compaction.md:425-459`,
  `settings-manager.ts:56`). In **1.4.0** `reserveTokens` "only decides when compaction starts; summary requests use the model's output limit"
  (`compaction.md:35`; CHANGELOG 1.4.0 "Summaries without a fixed output cap").
- **Format:** a structured summary (`## Goal`, `## Constraints & Preferences`, `## Progress`, `## Key Decisions`, `## Next Steps`,
  `## Critical Context`) `compaction/compaction.ts:507-575`, stored as a `CompactionEntry` with `firstKeptEntryId` (`compaction.md` "How It Works").
- **cpd's additions:** (1) the **ratio**: `pi/lib/compaction-ratio.ts` replaces the reserve with `round(contextWindow * ratio)` (default 0.1),
  per-model ratio overrides, by wrapping `SettingsManager.prototype.getCompactionReserveTokens` (`:67-69`, **read**); the prototype-patching is what
  `docs/research/prototype-patches.md` targets. (2) `pi/compact-tools/extensions/compact-tools-compaction.ts` redraws pi's compaction notice as a
  cpd fold (`:1-8`, **read**): pure UI.

### What pi-durable has

**Built in. pi-durable has compaction** (`pi:packages/durable/README.md` "## Compaction", `:550-588`; **ran** counts below show it spread over 16 files of `src`):

- **Automatic:** `settings.compaction = { enabled, reserveTokens (16384), keepRecentTokens (20000), backgroundTokens (32768), modelOverrides }`
  (`harness/agent.ts:27-32` `DEFAULT_COMPACTION_POLICY`, `types.ts:505-524` types). Above `contextWindow - reserveTokens` the next request **waits** for a compaction
  (`generation.ts:355`); `backgroundTokens` below that it starts **in the background** (`generation.ts:356`), which pi does not do. A provider
  context-overflow rejection compacts and retries once (README `:584`).
- **Manual:** `conversation.compact(instructions, context)` (`types.ts:747`); Esc aborts it.
- **Summary:** same structured format as pi (`compaction.ts:74-100` vs pi `compaction.ts:507`), summarisation spend counted in `pi.usage` (README `:584`).
- **Is a compaction a committed event? Yes.** The result is an entry `pi.compaction` (`CompactionEntry`, `entries.ts:34`) whose `model` is
  `[UserMessage]` with the summary wrapped in `<summary>` and whose `head` is the first kept entry; **older entries stay in storage** (README `:552`).
  The task itself is a durable task with checkpoints `select -> summarize -> retry` (`compaction.ts:55-58`), so a crash mid-summary resumes. A summary
  is placed at once when idle, else at the next turn boundary; one that would cut before the current context is `stale` (README `:563,:584`).
- **Hook:** `beforeCompact(compaction, api, ctx)` can `decline` or supply its own `summary` (`types.ts:890-905`). **Whether a section/hook can rewrite the
  request:** yes, `GenerationTask` `beforeRequest` replaces the messages of one request (`types.ts:853-858`); sections cannot see or change messages.
- **Search** (as asked), **ran** in `/tmp` clone: files in `packages/durable/src` / `packages/chord/src` matching `compact` 16 / 1, `summar` 9 / 3, `window`
  9 / 5, `truncat` 11 / 2. `chord` only holds types and a delta helper; the policy lives in durable.

### Gap and cost

| Gap | Fix | Size | Basis |
|---|---|---|---|
| cpd's **ratio** reserve (`round(window * ratio)`, per-model ratio) | `compactionPolicy` reads `settings.compaction.modelOverrides?.[provider/modelId]` (`compaction.ts:262-271`) and `settings` "is read at every use and never stored, so getters make it live" (README "Settings"). Pass a `modelOverrides` **Proxy** (or fill it from the catalog) computing `{reserveTokens: round(window*ratio), backgroundTokens}` from the model catalog. The Proxy is **unverified** (not run); the catalog fill is the safe variant | ~30 | `compaction-ratio.ts` 69 lines, no prototype hook and no throwing branch kept |
| The 1.4.0 output-limit fix for summaries | durable still caps summary output at `min(0.8 * reserveTokens, model.maxTokens)` (`compaction.ts:148-151`), the same as pi **1.3.0** (**ran** `grep -n reserveTokens node_modules/.../compaction/compaction.js:512`). With a ratio reserve of ~10% of a 1M window the cap is huge; with a 32k window it is small and a reasoning model can hit it (the CHANGELOG 1.4.0 symptom "generation hit the token cap"; unverified for durable). Fix = `beforeCompact` with a cpd summariser, or an upstream issue | 0 or ~80 | `beforeCompact` + `generateSummary` call; basis `compaction/compaction.ts:507-575` prompts are 70 lines (and can be imported from pi-ai `complete`) |
| `/compact` and the auto-compaction notice in the chat | `root.compact()`; render `compaction_start/_end` agent events (README `:588`) | ~20 | UI side in `parity-runtime`/`widgets` |
| Fold drawing (`compact-tools-compaction.ts`, 91 lines) | **delete**: a ratatui widget draws the `pi.compaction` entry | -91 | the file |
| Branch summaries (`/tree`) | not needed by cpd (no tree navigation in the chat) | 0 | `parity-runtime` |

**What an extension must build if nothing were built in:** not applicable; pi-durable's is complete for cpd's use. What the **Session file** loses is
`parity-runtime`'s question (cpd's metrics read pi's JSONL: `docs/research/pi-durable.md` item 7).

## Version differences met (pi 1.3.0 in cpd, 1.4.0 read)

| Area | 1.3.0 (installed copy in `node_modules`) | 1.4.0 (`v1.4.0`) | Evidence |
|---|---|---|---|
| Summary output cap | `min(floor(0.8 * reserveTokens), model.maxTokens)` | the model's output limit; `reserveTokens` only sets when to compact | **ran** `grep -n reserveTokens node_modules/@earendil-works/pi-coding-agent/dist/core/compaction/compaction.js` (line 512) vs `pi:CHANGELOG` 1.4.0 and `docs/compaction.md:35`. **Consequence:** cpd's ratio doubles as a summary cap on 1.3.0 today (unverified as an intent), and not on 1.4.0 |
| `sections` in `systemPromptOptions` | present | present | **ran** `grep -n sections node_modules/@earendil-works/pi-coding-agent/dist/core/extensions/types.d.ts` -> line 724 |
| Per-model compaction overrides | present (3 hits for `modelOverrides` in `dist/core/settings-manager.js`) | present | **ran** `grep -c modelOverrides` -> 3 |
| `Models`/`ModelRuntime` | present (cpd's helpers use them) | `ModelRuntime implements Models` | **read** `model-runtime.ts:174` |
| `/compact <instructions>` while a turn is split | instructions ignored in some cases | fixed (CHANGELOG 1.4.0) | `pi:CHANGELOG` |

## Where the earlier docs are stale or need a note

- `docs/research/pi-durable.md` ("What would have to be built"): says a durable Worker needs "one cpd extension: `section`s for the axioms ...". **Still right.**
  It is silent on compaction: **pi-durable has built-in compaction**, so compaction is a config item, not a build item.
- `docs/research/pi-durable.md` open question "Model login: unverified whether Pi Durable can use pi's `auth.json`": **answered**: pi-ai does not read `auth.json`
  (`git grep -n auth.json -- packages/ai/src` -> only `src/cli.ts:9`, a CLI helper, **ran**); pi-coding-agent's `ModelRuntime`/`AuthStorage` do, and `ModelRuntime`
  can be passed as durable's `models`.
- The Job text lists exposures as `direct`/`deferred`/`hidden`; the policy and pi have a fourth, `codemode`, which is pi's default.

## Does not need replacing (runner-neutral cpd logic)

- Pure functions of the adapter: `policySection` (`pi/lib/policy.ts`, 25), `zoneLine` and the prose-time functions (`pi/lib/time.ts`, 56), the guards'
  decision functions `primaryCheckoutMove/Write`, `secretBypass`, `hostApproveBypass`, `blockingReason`, `unfilteredTestRun`, `tooLong`, `unseen`, `pnVerdict`
  (**ran** `wc -l`: 253 + 23 + 60 + 84 + 85 + 104 + 40 = **649 lines**), `labelsIn`, `textLength`.
- The `cpd` CLI client `pi/lib/cpd.ts` (87), the Event decoder `pi/lib/event.ts` (98).
- The Rust side: Tier mapping and ladder (contract §9), `cpd tiers`, the `[model]` and `machine.mcp` policy (`core/src/policy/machine.rs`, 290), `cpd mcp attach`
  (`core/src/mcp.rs`, 75), the `mcp-attached` Event, `cpd policy show`.
- The helpers that already run outside pi: `models-list.ts` (E15), `slack-channel.ts`, `slack-post.ts`, `jira-transition.ts` (they load pi's `dist` by path, not as
  extensions). They keep working while pi is installed.
- The two skills' text (`pi/skills/cpd-lead`, `cpd-manager`): Markdown, loaded by the list section; their content refers to pi tools by name (unverified: a check for pi-only
  tool names such as `ask_user_question` is needed).

## Must be rebuilt for pi-durable

| What | Lines | Note |
|---|---|---|
| Skill list section | ~30 | calls pi's exported loaders (or a ~150-line port) |
| `AGENTS.md` / context-files section | ~20 | calls `loadProjectContextFiles` |
| `cpd-policy` and `cpd-time-zone` sections | ~40 | async `render`, existing pure functions |
| Guard glue: one `beforeTool` hook, role-selected | ~100 | mainguard 15, bash guards 40, askguard 40; `cwd` and transcript access unverified |
| `loop-guard` as a `GenerationTask` hook | ~50 | |
| MCP extension over `pi-mcp` | ~450 | the largest item |
| Compaction ratio provider | ~30 | |
| Retier + model setup | ~60 | `configure()` + `ModelRuntime` |
| **Total TypeScript, new** | **~780** (say 800; range 650-1000) | |
| Rust: a launcher for a durable runner (`fn manager`/`fn lead` variant, argv + env) | ~100 | basis `fn worker` in `launch.rs:165-190` (26 lines) times the three roles plus tests of `launch_no_mcp_tests.rs` style |

Deleted by the move (this half only): `pi/lib/compaction-ratio.ts` (69), `pi/compact-tools/extensions/compact-tools-compaction.ts` (91), the `--no-mcp` and
`registerMcpServer` plumbing `pi/lib/mcp.ts` (18) and `pi/mcp-extension.ts` (4): **182 lines**.

## Can be taken as a dependency (convention over configuration)

| Take | What it replaces | Version / pin | Evidence |
|---|---|---|---|
| `@earendil-works/pi-durable` + `pi-ai` + `chord` | the loop, sections, hooks, compaction, tool_search, codemode | `1.4.0` (the three versions are locked together, **ran** `npm view @earendil-works/pi-durable@1.4.0 dependencies`) | README |
| `@earendil-works/pi-mcp` | the MCP client and OAuth flow | `1.4.0`, one dependency `cross-spawn` | **ran** `npm view` |
| `@earendil-works/pi-coding-agent` (already a cpd peer dependency, `pi/package.json`) for `loadProjectContextFiles`, `loadSkills`, `formatSkillsForPrompt`, `ModelRuntime` | AGENTS.md, skills, models.json, auth.json | `1.4.0` for the durable runner; cpd's pi is 1.3.0: pin the same version on both or verify the exports exist in 1.3.0 (unverified: `loadSkills` and `loadProjectContextFiles` exports in 1.3.0) | `pi:packages/coding-agent/src/index.ts:248,325-329`, `core/model-runtime.ts:174` |
| `createToolSearchExtension`, `createCodemodeExtension`, `CodingTools` | `tool_search`, `codemode`, the four tools | in pi-durable | `types.ts`, README |
| `pi-codemode` (`quickjs-wasi`) | the codemode sandbox | `1.4.0` | **ran** `npm view` of dependencies (via durable) |

Packages searched and not taken: `pi-mcp-adapter`, `pi-mcp-extension` (pi coding agent only), `@parcha/pi-durable-disk`, `@ai-ecoverse/slicc-agent`
(app-level). **ran** `npm search --json "pi-durable"` and `"keywords:pi-durable"`.

## Size estimate (this half)

| Item | TS lines new | Rust lines new | Modules | Basis (all `wc -l`, below) |
|---|---|---|---|---|
| Prompt: skills + context files + policy + time | ~90 | 0 | 1 (`prompt.ts`) | `policy.ts` 25, `agent-state.ts:30-46` 17, `resource-loader.ts:232-274` 42 |
| Guards and tools: `beforeTool`, `loop-guard`, install tools | ~150 | 0 | 1 (`guards.ts`) | `adapter.ts:70-103` 34, `loop-guard.ts` 60 |
| MCP extension | ~450 | 0 | 1 (`mcp.ts`) | `runtime.ts` 454 + `tools.ts` 391 + `config.ts` 243, about half |
| Compaction (ratio provider, `/compact`) | ~50 | 0 | 1 (`compaction.ts`) | `compaction-ratio.ts` 69 |
| Models, auth, retier | ~60 | 0 | 1 (`models.ts`) | `retier.ts` 73, `ModelRuntime` reused |
| Runner launch (`fn manager`/`lead` for the durable runner) | 0 | ~100 | 1 (`launch.rs`) | `fn worker` 26 lines; `launch.rs` 548 |
| **Total** | **~800** (650-1000) | **~100** | **6** | |
| **PRs** | 4: (1) prompt sections + AGENTS/skills, (2) guards + tools, (3) models/auth + compaction, (4) MCP | 1, shared with the `parity-runtime` launcher | | each ~150-450 lines |

Risks and what is unverified in this half: (1) the `modelOverrides` Proxy for the ratio; (2) `cwd` and transcript access from `HookApi` for mainguard and askguard;
(3) whether a durable extension can read `mcp-auth.json`; (4) type-compatibility of `ModelRuntime` as `Models` at run time; (5) the 1.3.0 versus 1.4.0 exports. Each can be
verified with a small probe program on pi-durable's faux provider (`pi:packages/ai/src/providers/faux.ts`, used by `test/examples/26-coding-agent.ts`), which this Job
was not allowed to run.

## Commands run

All run from `/tmp/pc1` (clone) or this worktree, each wrapped in `timeout`; the clone was removed at the end.

```sh
git clone --depth 1 --branch v1.4.0 https://github.com/earendil-works/pi /tmp/pc1/pi-src     # commit 4a080252, 2026-10-22 00:29 +02:00
npm view @earendil-works/pi-mcp@1.4.0 dependencies peerDependencies        # { 'cross-spawn': '7.0.6' }
npm view @earendil-works/pi-durable@1.4.0 dependencies                     # chord, pi-ai, pi-codemode, pi-telemetry, photon, diff, typebox
git grep -n "pi-agent-core\|pi-coding-agent" -- packages/mcp/src          # no output
git grep -il "mcp" -- packages/durable/src packages/durable/package.json   # 5 files, comments/prefix examples
git grep -in "skill" -- packages/durable/src packages/durable/README.md    # env-conformance.ts only
git grep -n "AGENTS" -- packages/durable/src packages/durable/README.md    # env-conformance.ts only
git grep -n 'grep\|"find"\|"ls"' -- packages/durable/src packages/durable/package.json   # no output
for w in compact summar window truncat; do git grep -il "$w" -- packages/durable/src | wc -l; done   # 16 9 9 11 (chord: 1 3 5 2)
grep -n "createMcpExtension\|McpServerConnection" packages/coding-agent/src/index.ts   # line 413: createMcpExtension only
wc -l <files>   # counts quoted in the tables
```

`wc -l` counts used: pi `skills.ts` 514; `system-prompt.ts` 229; `compaction/compaction.ts` 1104; durable `harness/compaction.ts` 483, `tool-search/tool-search.ts` 92,
`codemode/codemode.ts` 1044, `tools/{read,write,edit,bash}.ts` 312/39/144/184; coding-agent `extensions/mcp/{index,runtime,config,oauth,tools,resources,cli}.ts`
1347/454/243/558/391/339/594, `core/mcp-servers.ts` 346, `core/auth-storage.ts` 507; `packages/mcp/src/client.ts` 621 (whole `src` 3277); `ai/src/auth/credential-store.ts` 67;
examples `15-system-prompt` 93, `25-compaction` 123, `26-coding-agent` 81, `27-plan-mode` 116, `30-tool-override` 102; cpd `pi/lib/{mainguard 253, noblock 84, testguard 85,
secretguard 23, hostapprove 60, askguard 104, pnguard 40, loop-guard 60, retier 73, models-list 177, policy 25, time 56, mcp 18, compaction-ratio 69, agent-state 92}.ts`,
`pi/skills/cpd-lead/SKILL.md` 409, `cpd-manager/SKILL.md` 143, `compact-tools-compaction.ts` 91, `core/src/mcp.rs` 75, `core/src/policy/machine.rs` 290, `launch.rs` 548.
