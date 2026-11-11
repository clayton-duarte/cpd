# pi 1.5.2 compatibility for the cpd durable worker and MCP adapter

Job `cpd-adapter-docs` (Attempt `cpd-adapter-docs-a1`, Plan `pi-152`, Tier H3). Written 2026-11-10 16:58 EST. This file changes no code and no `package.json`. It records what pi 1.5.2 changes for cpd, what the adapter must change before the bump, and the version matrix. Scout source: `run/pi-152/explore/scout-adapter.md`.

## Answer first

1. **The bump breaks durable MCP.** pi 1.5.2 no longer exports `createMcpToolName` from `dist/extensions/mcp/tools.js`. cpd's `pi/durable-worker/pi-mcp.ts` loads that file and throws `does not export createMcpToolName` before any MCP connection is made, so every durable `mcp attach` fails with `mcp_register_failed`. Verified against the 1.5.2 tarball.
2. **The fix is an import location change, not a behaviour change.** In 1.5.2 the function comes from `@earendil-works/pi-mcp`. The adapter must import it from that package (see §1 for the options). The pi-mcp 1.5.2 package entry is **unverified** (its tarball is not in the local npm cache).
3. **The test that should catch this does not.** `pi/test/durable-worker/pi-mcp.test.ts` writes a fake pi with the 1.5.0 layout, so it passes on the broken code. Its fake must move to the 1.5.2 layout first, so the test fails for the right reason (§3).
4. **Everything else cpd loads from pi still exists in 1.5.2** (§2), and every `ExtensionAPI` member in `PiApi` is still declared (§3). No other adapter code change is required by the verified evidence.
5. **Decision for the Lead:** whether `@earendil-works/pi-durable` and `@earendil-works/pi-ai` move to 1.5.2 with pi. Until they do, the durable runner keeps the 1.3.0-era request shape (§4, row pi-ai).

## Evidence and method

- **Read:** `@earendil-works/pi-coding-agent-1.5.2.tgz` from the local npm cache (`~/.npm/_cacache`, the tarball the scout identified), extracted to a temporary directory and removed after reading. Its `package.json` depends on pi-ai, pi-agent-core, pi-codemode, pi-mcp and pi-tui at 1.5.2, and on typebox at 1.3.27.
- **Read:** cpd's installed pi 1.5.0 in `node_modules/@earendil-works/` (the repo's current pin) for the "before" side.
- **Read:** `/tmp/cpd-cpd/pi-changelog-1.4.0-1.5.2.md`.
- **Not available locally, so not read:** pi-mcp 1.5.x, pi-tui 1.5.2, pi-ai 1.5.2, pi-durable 1.5.1 and 1.5.2 (the cache holds pi-ai 1.5.0, pi-durable 1.5.0 and pi-codemode 1.5.0 only). Anything about them is labelled unverified.
- **Not run:** no test, no typecheck, no pi process, no model call. This change is documentation only.

## 1. `createMcpToolName`: where it lives

| pi version | Export location | Evidence |
|---|---|---|
| 1.5.0 (cpd `node_modules`) | `dist/extensions/mcp/tools.js` exports `createMcpToolName` | verified (`grep` of the installed file) |
| 1.5.2 | **not** in `dist/extensions/mcp/tools.js`. Its exports are `createMcpResultSchema`, `createMcpToolDefinition`, `createMcpToolRenderers`, `saveToTempFile`, `toToolExposure`, `READ_MCP_RESOURCE_TOOL`. | verified (tarball) |
| 1.5.2 | `dist/extensions/mcp/index.js:29` reads `import { createMcpToolName, mcpNamespace } from "@earendil-works/pi-mcp";` | verified (tarball) |
| 1.5.2 | The implementation is in the pi-mcp code bundled into `dist/bundle/chunks/chunk-4KSTFUIK.js`, exported as `createMcpToolName` and `mcpNamespace`. Its signature is `createMcpToolName(server, tool, isTaken = () => false)`. | verified (tarball, bundled copy) |
| 1.5.2 | The public `dist/index.d.ts` does not export it. | verified (tarball `grep`) |
| 1.5.2 | Not mentioned in the changelog 1.4.0 to 1.5.2. The move is undocumented. | verified (changelog `grep`) |

Behaviour of the function, as in the bundled copy (cpd uses it to name MCP tools in `pi/durable-worker/pi-mcp.ts:42`): the name is `mcp__<server>__<tool>` with every character outside `[A-Za-z0-9_]` replaced by `_`. If that is longer than 64 characters or `isTaken` returns true, it is cut and suffixed with `_` and the first 8 hex digits of `sha256("<server>\0<tool>")`. The fake in `pi/test/durable-worker/pi-mcp.test.ts` uses a different collision suffix (`_h4sh`). No test may assert on a collision name against the fake, only against the real algorithm.

**cpd's current code** (`pi/durable-worker/pi-mcp.ts`):
- `loadToolName` (lines 6 to 17) imports `extensions/mcp/tools.js` by file URL and throws `does not export createMcpToolName`.
- It is called on line 24, before `new pi.McpServerConnection` (line 25), so no connection opens on failure.
- The failure path is `pi/durable-worker/mcp.ts:50-61`: it reports `mcp_register_failed` to the Session and submits a follow-up to the Lead.

**Options for the fix** (the choice is the Lead's; this file does not make it):
- **A (recommended by the axioms, "take the dependency"):** add `@earendil-works/pi-mcp` pinned to `1.5.2` as a direct dependency, the same way the other pi packages are pinned, and import `createMcpToolName` by package name. It is explicit, and a package bump moves it with pi. Needs a `package.json` and lockfile change, out of scope for this Attempt. Unverified: that the pi-mcp 1.5.2 package entry exports the name (its tarball is not cached).
- **B:** keep the file-URL approach and point it at pi's own pi-mcp install (`node_modules/@earendil-works/pi-mcp` next to pi's install). Implicit: it depends on how npm hoists the package. Not recommended.

cpd's `node_modules` has no `@earendil-works/pi-mcp` today (verified: `ls node_modules/@earendil-works/`), because pi 1.5.0 has the function in its own tree. So any option needs pi-mcp installed.

## 2. Other pi names cpd loads by file URL (`pi/lib/slack-channel.ts`)

`loadPi` checks each name at load. All of them still exist in 1.5.2 (verified against the tarball):

| cpd name | cpd constant | 1.5.2 file | Status |
|---|---|---|---|
| `loadMcpConfig` | `PI_NAMES.config` | `dist/extensions/mcp/config.js` | present |
| `McpServerConnection` | `PI_NAMES.runtime` | `dist/extensions/mcp/runtime.js` | present |
| `McpOAuthCredentialStore` | `PI_NAMES.runtime` | `dist/extensions/mcp/runtime.js` | present |
| `createDefaultTransport` | `PI_NAMES.runtime` | `dist/extensions/mcp/runtime.js` | present |
| `createMcpToolName` | (loaded in `pi-mcp.ts`, not in `PI_NAMES`) | `dist/extensions/mcp/tools.js` | **removed**, see §1 |

Because `PI_NAMES` does not include `createMcpToolName`, the `slack-channel` check does not catch the break. Only `pi-mcp.ts` does.

## 3. FakePi and the test harness

- **`FakePi`** (`pi/test/fake.ts`, `implements PiApi`) mirrors `PiApi` (`pi/lib/pi.ts`), which is `Pick<ExtensionAPI, …>` over 15 members: `on`, `events`, `exec`, `sendMessage`, `appendEntry`, `setSessionName`, `getSessionName`, `registerMessageRenderer`, `registerTool`, `registerCommand`, `sendUserMessage`, `setModel`, `setThinkingLevel`, `registerMcpServer`. All 15 appear as declarations in 1.5.2's `dist/core/extensions/types.d.ts` (`interface ExtensionAPI` at line 1228, `events` at line 1451; checked by name). **No FakePi change is needed for 1.5.2.** Verified by name, not by a typecheck (that needs 1.5.2 installed; the `typecheck` run is part of the Gate).
- **The fake that does need to change** is the pi tree written by `pi/test/durable-worker/pi-mcp.test.ts` (`setup()`, line 43). It writes `FAKE_TOOLS` (`createMcpToolName` exported from `extensions/mcp/tools.js`), which is the 1.5.0 layout. The 1.5.2 layout is: `tools.js` has no `createMcpToolName`, and the name comes from a `@earendil-works/pi-mcp` module. Order of work for the fix, so the test fails for the right reason first: (1) rewrite the fake to the 1.5.2 layout, (2) run the test and see it fail with `does not export createMcpToolName`, (3) change the loader, (4) see it pass.
- **Real pi in tests (not a 1.5.2 change, but on the same path):** `pi/test/mcp-harness.ts` and three `compact-tools` tests start a real in-process pi SDK session (`createAgentSession`; the scout counted 4 files in `pi/test`). Their header says "real pi 1.3.0", which is stale (the repo pins 1.5.0). They run pi's own code, so they will see 1.5.2 rendering and MCP changes. Not changed here; the policy question is open (see §6).

## 4. Version compatibility matrix: pi 1.5.2 in cpd

"cpd pin" is `package.json` on `main`. "pi 1.5.2" is the version in the 1.5.2 tarball's dependencies or the file read.

| Component | cpd pin (main) | pi 1.5.2 | Status for cpd | Evidence |
|---|---|---|---|---|
| `@earendil-works/pi-coding-agent` | 1.5.0 | 1.5.2 | **Breaks durable MCP** (`createMcpToolName` moved, §1). Other names cpd loads are present (§2). | verified (tarball) |
| `@earendil-works/pi-mcp` | not a cpd dependency; absent from `node_modules` | 1.5.2 (dependency of pi-coding-agent) | Needed for the fix (§1 option A). Package entry unverified. | verified (dependency list); entry unverified (not cached) |
| `@earendil-works/pi-ai` | 1.5.0 | 1.5.2 | Two copies after a bump: pi-coding-agent 1.5.2 uses pi-ai 1.5.2, while cpd's root pins 1.5.0, which the durable runner loads (scout §3). The `tools: []` change is in 1.5.2's CLI path (below). | verified (dependency list, bundle); durable side unverified |
| `@earendil-works/pi-durable` | 1.5.0 (protocol-2 pin, commit `855527553`) | not a dependency of pi-coding-agent 1.5.2 | **Decision for the Lead** whether it moves. 1.5.1 and 1.5.2 tarballs not cached. | verified (no dependency); 1.5.x unverified |
| `@earendil-works/pi-tui` | `^1.5.0` (resolves 1.5.0) | 1.5.2 | Used for `Component` types and overlays. 1.5.1 changed overlay disposal on `/reload` (§5). | verified (dependency); behaviour unverified |
| `@earendil-works/pi-codemode` | 1.5.0 | 1.5.2 (dependency) | `codemode` still takes `code` (`dist/extensions/codemode/tool.js`, verified). The package itself unverified. | partly verified |
| `@earendil-works/chord` | 1.5.0 | not checked | Unverified. | none |
| `typebox` | 1.3.27 | 1.3.27 | Same version. Tool argument validation moved to `@earendil-works/pi-schema` in 1.5.0 (changelog). cpd's tool schemas are plain JSON Schema or TypeBox features that survive it. | verified (versions); schema compliance from scout probes, not re-run |
| Custom `GrepOperations` | none | `readFile` no longer called for context lines (1.5.1) | No effect: no cpd code calls `createGrepTool` with operations. | verified (repo `grep`, zero results) |
| Tool order in `setActiveTools` (#11495) | cpd appends (`reconcile.ts:35,37`) | keeps existing order, appends new | No effect; the change removes spurious prompt rewrites. | verified (cpd lines read); behaviour from scout |
| Installed user pi | not in repo | `~/.pi/agent/install/releases/` holds 1.2.0 and 1.3.0 only | Not 1.5.2 on this machine. | verified (`ls`) |

**Bifrost and `tools: []` (changelog 1.5.1, Breaking Changes):** OpenAI-compatible models no longer send `"tools": []` after tool history when no tools are enabled. In 1.5.2 the CLI path does this (`dist/bundle/chunks/openai-completions-HRZOSLBO.js`, `compat.requiresToolsWithToolHistory && hasToolHistory(...) && (params.tools = [])`). pi-ai 1.5.0 still sends `tools: []` after tool history (`node_modules/@earendil-works/pi-ai/dist/api/openai-completions.js:592`). cpd sends no request with zero tools and tool history: Lead, Manager, Worker and the durable runner always declare codemode, and the Distiller runs with no tool history (scout §3). **No cpd change needed. Whether a Bifrost gateway needs `compat.requiresToolsWithToolHistory: true` is unverified** (needs a live call).

## 5. Behaviour changes that need no adapter change

- **1.5.0 "Tightened system prompt":** cpd sets no `customPrompt` or `systemPrompt`, and no cpd text quotes the changed preamble or tools trailer (scout §4). The effect on model behaviour is **unverified**.
- **1.5.1 overlays on `/reload`:** reload and session switches now dispose all extension overlays, including hidden ones. cpd's `ask_user_question` overlay relies on `OverlayHandle.setHidden` and a `Promise.race` bound. Whether a pending question settles across `/reload` on 1.5.2 is **unverified**. Check it before shipping (scout §6).
- **1.5.0 extension API:** every name cpd imports from `@earendil-works/pi-coding-agent` is declared in 1.5.2 (scout, static check). The `tsc` run against 1.5.2 types is the real check; it has not been run.

## 6. Unverified, and open risks

- The pi-mcp 1.5.2 package entry: whether `@earendil-works/pi-mcp` exports `createMcpToolName` from its public entry (option A depends on it). Not cached.
- pi-durable 1.5.1 and 1.5.2, pi-ai 1.5.2, pi-tui 1.5.2, pi-codemode 1.5.2 full source: not cached.
- Whether the durable runner's pi-ai 1.5.0 is acceptable after the bump: two pi-ai copies (§4).
- Model-facing effect of the 1.5.0 prompt wording and the `tools: []` change: needs model runs.
- Real-pi render and MCP tests (§3): they run pi's own 1.5.x code, outside the stated rule that tests mock pi. The policy call is the Lead's.

## 7. Next steps (ordered, for the adapter Job; decisions marked)

1. **Decision:** option A or B for `createMcpToolName` (§1). Recommended: A.
2. **Red test:** rewrite the fake in `pi/test/durable-worker/pi-mcp.test.ts` to the 1.5.2 layout and confirm it fails with `does not export createMcpToolName`.
3. **Fix:** change `pi/durable-worker/pi-mcp.ts` to the chosen import; confirm the test passes.
4. **Decision:** whether pi-durable and pi-ai move to 1.5.2 with pi (§4).
5. **Bump and check:** install 1.5.2, run `typecheck` (the Gate's first step), and check the overlay case in §5.
