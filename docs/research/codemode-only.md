STOP: under `codemode.mode: only` pi hides `ask_user_question` from the model (it is `direct`); `model-only` tools stay declared. `.pi/settings.json` was not written.

# codemode.mode only: what it hides (hs-d737)

Labels: `read` = read in source, `ran` = ran with no model, `unverified` = not checked.

## What pi does (`read`)

`node_modules/@earendil-works/pi-coding-agent/dist/extensions/codemode/tool.js`, `prepareCodemodeLoadout`: with `mode: "only"`, `hiddenDeclarations` is every callable tool that has `direct` exposure and is declared. With `on` it is `[]`. `dist/core/agent-session.js` (`_applyToolLoadout`, `_installHiddenDeclarationsProjection`) removes those names from every request; `agent.state.tools` still lists them. `codemode` itself and `model-only` tools are not callable from scripts, so they are never in `callable` and never hidden. A tool with no `exposure` is `direct` (`_getToolExposure`), so `ask_user_question` (pi/ask-user-question/ask-user-question.ts, no `exposure`) is hidden under `only`.

## Result (`ran`)

"Declared" = in `session.agent.state.tools` minus `session._hiddenDeclarations` (a private field; `state.tools` alone does not show the hiding). "Reachable" = in `ALL_TOOLS` of a `codemode` script. pi 1.3.0, Node v22.23.2, `ask_user_question` loaded from the real `pi/ask-user-question/index.ts` (its factory registers only with `CPD_SESSION` set and `CPD_ATTEMPT` unset, which the script sets in its own process; no cpd session is started).

| tool | declared, `on` | reachable, `on` | declared, `only` | reachable, `only` |
|---|---|---|---|---|
| read | yes | yes | no | yes |
| bash | yes | yes | no | yes |
| edit | yes | yes | no | yes |
| write | yes | yes | no | yes |
| ask_user_question | yes | yes | **no** | yes |
| model-only stub (stands for cpd_plan_review, cpd_secret_approve, cpd_host_approve) | yes | no | yes | no |
| codemode | yes | no | yes | no |

Verdict rule (Job): `only` is set only if `ask_user_question` and the model-only stub both stay declared. The stub does, `ask_user_question` does not: STOP. The Lead decides (for example `exposure: "model-only"` on cpd's copy of `ask_user_question`).

## Not checked (`unverified`)

- The plannotator tools (external package): not loaded; their exposure is unknown, so whether `only` hides them is unverified.
- The real `cpd_plan_review`, `cpd_secret_approve`, `cpd_host_approve`: not loaded; a stub stands for them. Their `exposure: "model-only"` is `read` in pi/lib, so by the rule above they stay declared.
- That `.pi/settings.json` loads only when the project is trusted, and the mac-pro's `defaultProjectTrust`: `unverified` (docs say project settings load after trust).
- Whether a model could ask the User by calling `tools.ask_user_question` from a script (it is in `ALL_TOOLS`): not run; `unverified`.

## The throwaway script

Ran as `WT=<worktree> timeout 120 node $(mktemp -d)/probe.mjs`; the directory and its temp home were deleted afterwards.

```js
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
const W = process.env.WT + "/node_modules/@earendil-works/pi-coding-agent/";
const { createAgentSession, DefaultResourceLoader, SessionManager, SettingsManager } = await import(W + "dist/index.js");
const native = await import(W + "dist/extensions/codemode/index.js");
process.env.CPD_SESSION = "probe"; delete process.env.CPD_ATTEMPT; // makes cpd's ask extension register; no cpd session is started
const ask = await import(process.env.WT + "/pi/ask-user-question/index.ts");
const dir = mkdtempSync(join(tmpdir(), "probe-home-"));
const stub = (pi) => pi.registerTool({ name: "model_only_stub", label: "stub", description: "stub for cpd_plan_review, cpd_secret_approve, cpd_host_approve", parameters: { type: "object", properties: {} }, exposure: "model-only", execute: async () => ({ content: [{ type: "text", text: "x" }], details: {} }) });
const names = ["read", "bash", "edit", "write", "ask_user_question", "model_only_stub", "codemode"];
for (const mode of ["on", "only"]) {
  const settingsManager = SettingsManager.inMemory({ defaultTools: ["read", "bash", "edit", "write", "codemode"], codemode: { mode } });
  const loader = new DefaultResourceLoader({ cwd: dir, agentDir: dir, settingsManager, noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true, extensionFactories: [native.default, ask.default, stub] });
  await loader.reload();
  const { session } = await createAgentSession({ cwd: dir, agentDir: dir, settingsManager, resourceLoader: loader, sessionManager: SessionManager.inMemory(dir) });
  await session.bindExtensions({ mode: "print", onError: (e) => { throw new Error(e.error); } });
  const stateTools = session.agent.state.tools.map((t) => t.name);
  session._applyToolLoadout(stateTools); const hidden = [...session._hiddenDeclarations]; // private field: what pi removes from each request
  const declared = stateTools.filter((n) => !hidden.includes(n));
  const tool = session.agent.state.tools.find((t) => t.name === "codemode");
  const id = "p-" + mode, args = { code: "text(JSON.stringify(ALL_TOOLS.map(t => t.name)))" };
  session.agent.state.messages = [{ role: "assistant", content: [{ type: "toolCall", id, name: "codemode", arguments: args }], api: "anthropic-messages", provider: "anthropic", model: "t", stopReason: "toolUse", timestamp: Date.now(), usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } }];
  const r = await tool.execute(id, args, new AbortController().signal);
  const raw = r.content.map((c) => c.text).join(" | "); console.log("raw:", raw); const reach = (raw.match(/\[.*\]/s) || ["[]"])[0]; const reachA = JSON.parse(reach);
  console.log(`mode ${mode}: state.tools=${JSON.stringify(stateTools)} hidden=${JSON.stringify(hidden)}`);
  console.log(`mode ${mode}: declared(all)=${JSON.stringify(declared)}`);
  console.log(`mode ${mode}: reachable(all)=${reach}`);
  for (const n of names) console.log(`  ${n}: declared=${declared.includes(n)} reachable=${reachA.includes(n)}`);
  session.dispose();
}
rmSync(dir, { recursive: true, force: true });
```

## Output

```
mode on: state.tools=["read","bash","edit","write","codemode","ask_user_question","model_only_stub"] hidden=[]
mode on: declared(all)=["read","bash","edit","write","codemode","ask_user_question","model_only_stub"]
mode on: reachable(all)=["read","bash","edit","write","ask_user_question"]
  read: declared=true reachable=true
  bash: declared=true reachable=true
  edit: declared=true reachable=true
  write: declared=true reachable=true
  ask_user_question: declared=true reachable=true
  model_only_stub: declared=true reachable=false
  codemode: declared=true reachable=false
mode only: state.tools=["read","bash","edit","write","codemode","ask_user_question","model_only_stub"] hidden=["read","bash","edit","write","ask_user_question"]
mode only: declared(all)=["codemode","model_only_stub"]
mode only: reachable(all)=["read","bash","edit","write","ask_user_question"]
  read: declared=false reachable=true
  bash: declared=false reachable=true
  edit: declared=false reachable=true
  write: declared=false reachable=true
  ask_user_question: declared=false reachable=true
  model_only_stub: declared=true reachable=false
  codemode: declared=true reachable=false
```
