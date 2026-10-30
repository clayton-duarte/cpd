# codemode as cpd's default: guards, excluded tools, compact rows (hs-d736)

pi 1.3.0 (`node_modules/@earendil-works/pi-coding-agent/package.json` version; `pi/` paths below are repo-relative, `dist/`/`docs/` are under that package). Labels: *read* = source read, *ran* = executed with no model, `unverified` = not checked.
No NO answers, so no `Contradiction` section: A4 holds.

## Q1. YES: a codemode script's calls reach every `tool_call` handler with `parentToolCallId`, and `{ block: true }` stops the call

- *read* `dist/core/agent-session.js:327-345` `_beforeToolCall` emits `tool_call` with `...(parentToolCallId ? { parentToolCallId } : {})`; `:388-405` `_executeNestedToolCall` runs `runToolCall` with `beforeToolCall: (context) => this._beforeToolCall(context, parentId)` (and `afterToolCall` likewise), the same pipeline as model calls.
- *read* `dist/core/extensions/runner.js:957-969` `emitToolCall` loops over every extension's handlers and returns at the first `result.block`.
- *read* `dist/extensions/codemode/execute.js:421` scripts call `ctx.executeTool(tool.name, args, ...)`; `:422-428` an `isError` outcome becomes an error record.
- *read* `docs/extensions.md:149` (nested calls go through `tool_call`/`tool_result` handlers, ids `<parent id>/<n>`); `docs/mcp.md:252` (codemode calls carry the codemode call ID as `parentToolCallId`).
- *ran* (script below, case `q1`): handler records `read`, `bash`(marker), `bash`(plain); the marker call is blocked.
  - events: `[{"toolName":"read","toolCallId":"probe-q1/1","parentToolCallId":"probe-q1"},{"toolName":"bash","toolCallId":"probe-q1/2","parentToolCallId":"probe-q1"},{"toolName":"bash","toolCallId":"probe-q1/3","parentToolCallId":"probe-q1"}]`
  - script output: `{"read":"ok","blocked":"rejected: blocked by probe guard","allowed":"{\"output\":\"fine\\n\",\"truncated\":false,\"exit_code\":0,...}"}`: the blocked `tools.bash()` rejected with the guard's exact reason; the plain call ran.
- Consequence for `pi/lib/adapter.ts:59`: the one `tool_call` handler is a registered extension handler, so it sees `edit`/`write`/`bash`/`ask_user_question` calls made inside scripts with `toolName` set to the nested tool; its existing checks on `event.toolName` and `event.input` apply unchanged. Not run against the real adapter (`unverified`); only the pi mechanism was run. Note the handler also sees the outer `codemode` call (`toolName: "codemode"`, no parent) when a model issues it; my script called `execute` directly, so that outer event is absent from the events above.

## Q2. YES: with `--exclude-tools X` a codemode script cannot call X

- *read* `dist/cli/args.js:127-128` parses `--exclude-tools`/`-xt`; `dist/main.js:436-437` sets `options.excludeTools`, `:686` passes it on; `dist/core/sdk.js:158-160` builds the matcher and filters `initialActiveToolNames`; `:317` passes `excludedToolNames` to `AgentSession`.
- *read* `dist/core/agent-session.js:198` `_excludedTools`; `:1123-1128` `_isAllowedTool` returns false first for an excluded name; `:2841-2856` `_refreshToolRegistry` filters custom/extension tools (`:2854`) and base tools (`:2856`) through `_isAllowedTool`, so an excluded tool is not in `_toolRegistry`; `:1148-1154` `_getCallableTools` reads only `_toolRegistry`, so an excluded tool is never callable (whatever its exposure).
- *read* `docs/cli.md:121-122`: `--exclude-tools` "disables ... after all other selection options". `docs/cli.md:128`: `--tools +codemode,-write` adds to the default selection. Combination in code: `sdk.js:150-160` applies `+codemode` to the defaults first, then removes excluded names from the active set; `_isAllowedTool` still drops them from the registry. So `--tools +codemode` with `--exclude-tools ...` behaves as the docs say. The combined real CLI argv was not run (`unverified`); the SDK path `excludeTools` that `main.js:686` feeds was run.
- *ran* (script below, cases `q2-control` and `q2-excluded`; `write` is a built-in, `ask_user_question` a `customTools` tool):
  - control, no `excludeTools`: `ALL_TOOLS` = `read,bash,edit,write,ask_user_question`; `tools.write` and `tools.ask_user_question` both `called`.
  - `excludeTools: ["write","ask_user_question"]`: `ALL_TOOLS` = `read,bash,edit`; both reject with `tools.write does not exist. Available: read, bash, edit. ...` (same for `ask_user_question`). `getCallableToolNames()` = `["read","bash","edit"]`.
- Worker consequence: `WORKER_EXCLUDED_TOOLS` (`core/src/host/pi/launch.rs:14`, passed as `--exclude-tools` at `:166`) keeps `ask_user_question`, `cpd_plan_review`, `plannotator*` out of scripts too. Pattern entries (`*`) go through the same matcher (read `docs/cli.md:121`); only exact names were run.

## Q3. YES: a codemode call still renders as a compact row with its nested summary

- *ran* `timeout 600 node --test pi/test/compact-tools/codemode-host.test.ts pi/test/compact-tools/codemode-summary.test.ts pi/test/compact-tools/codemode-title-style.test.ts`: exit 0, 39 tests, 39 pass, 0 fail. Per file (run separately): `codemode-host.test.ts` 3/3, `codemode-summary.test.ts` 34/34, `codemode-title-style.test.ts` 2/2.
- `codemode-host.test.ts` loops over the styles `compact`, `claude` and `codex` (line 19), one test each. Each builds a real in-process pi session (`createAgentSession`, in-memory settings with `defaultTools: ["read","bash","edit","codemode"]`), loads pi's native `dist/extensions/codemode/index.js` plus `pi/compact-tools/extensions/compact-tools.ts`, drives the `codemode` tool's `execute` directly (no model, no provider), draws a `ToolExecutionComponent` and asserts the row, including after `session.reload()` and after clearing the summary cache.
- Drawn row (assertion at `codemode-host.test.ts:66`, regex `/Done in 1\.234s.*read 3 · bash 1 · edit 1 1/u`, passing for all three styles); golden `pi/test/compact-tools/golden/codemode-summary.json:5`: ` └ Done in 1.234s (5 lines) · read 3 · bash 1 · edit 2`.
- Not covered: a live Lead pane in tmux (`unverified`); the tests draw the component, not a terminal.

## The throwaway script

Ran as `WT=<worktree> timeout 120 node $(mktemp -d)/probe.mjs`, with Node v22.23.2; the directory and its temp home were deleted afterwards. It imports pi's `dist/index.js` by absolute path and uses `customTools`, `excludeTools` and an extension factory (`recorder`).

```js
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
const W = process.env.WT + "/node_modules/@earendil-works/pi-coding-agent/";
const { createAgentSession, DefaultResourceLoader, SessionManager, SettingsManager } = await import(W + "dist/index.js");
const native = await import(W + "dist/extensions/codemode/index.js");
const dir = mkdtempSync(join(tmpdir(), "probe-home-"));
writeFileSync(join(dir, "a.txt"), "one\n");
const events = [];
const recorder = (pi) => pi.on("tool_call", async (e) => {
  events.push({ toolName: e.toolName, toolCallId: e.toolCallId, parentToolCallId: e.parentToolCallId });
  if (e.toolName === "bash" && String(e.input.command).includes("FORBIDDEN")) return { block: true, reason: "blocked by probe guard" };
});
const askTool = { name: "ask_user_question", label: "ask", description: "probe", parameters: { type: "object", properties: {} }, exposure: "direct", execute: async () => ({ content: [{ type: "text", text: "asked" }], details: {} }) };
async function run(label, excludeTools, code) {
  events.length = 0;
  const settingsManager = SettingsManager.inMemory({ defaultTools: ["read", "bash", "edit", "write", "codemode"] });
  const sessionManager = SessionManager.inMemory(dir);
  const loader = new DefaultResourceLoader({ cwd: dir, agentDir: dir, settingsManager, noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true, extensionFactories: [native.default, recorder] });
  await loader.reload();
  const { session } = await createAgentSession({ cwd: dir, agentDir: dir, settingsManager, resourceLoader: loader, sessionManager, customTools: [askTool], ...(excludeTools ? { excludeTools } : {}) });
  await session.bindExtensions({ mode: "print", onError: (e) => { throw new Error(e.error); } });
  const id = "probe-" + label;
  const args = { code };
  session.agent.state.messages = [{ role: "assistant", content: [{ type: "toolCall", id, name: "codemode", arguments: args }], api: "anthropic-messages", provider: "anthropic", model: "t", stopReason: "toolUse", timestamp: Date.now(), usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } }];
  const tool = session.agent.state.tools.find((t) => t.name === "codemode");
  const result = await tool.execute(id, args, new AbortController().signal);
  console.log(`--- ${label} (excludeTools=${JSON.stringify(excludeTools)})`);
  console.log("callable:", JSON.stringify(session.getCallableToolNames()));
  console.log("script output:", result.content.map((c) => c.text).join(" | "));
  if (label === "q1") console.log("tool_call events:", JSON.stringify(events));
  session.dispose();
}
const q2 = `const names = ALL_TOOLS.map(t => t.name); const out = {names};
for (const n of ["write","ask_user_question"]) { try { out[n] = typeof tools[n] === "function" ? (await tools[n](n === "write" ? {path:"w.txt", content:"x"} : {})) && "called" : "no tools."+n; } catch (e) { out[n] = "rejected: " + e.message; } }
text(JSON.stringify(out));`;
await run("q2-control", undefined, q2);
await run("q2-excluded", ["write", "ask_user_question"], q2);
const q1 = `const out = {};
out.read = (await tools.read({path:"a.txt"})) ? "ok" : "?";
try { await tools.bash({command:"echo FORBIDDEN"}); out.blocked = "ran"; } catch (e) { out.blocked = "rejected: " + e.message; }
try { const r = await tools.bash({command:"echo fine"}); out.allowed = JSON.stringify(r); } catch (e) { out.allowed = "rejected: " + e.message; }
text(JSON.stringify(out));`;
await run("q1", undefined, q1);
rmSync(dir, { recursive: true, force: true });
```

Full run output is quoted per question above.
