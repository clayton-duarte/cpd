# How the pi adapter can reload pi by itself

Job `reload-research` of Plan `cross-repo` (hs-d141, hs-n53, hs-d49). Question: after cpd is reinstalled, how can the adapter make a running pi session do what `/reload` does (extensions, skills, prompts, themes, context files, keybindings) with no User typing and nothing typed into the pane. This file changes no code.

## Method and evidence

- pi **1.2.0**, read at `~/.pi/agent/install/releases/1.2.0/node_modules/@earendil-works/pi-coding-agent/`. `dist/` paths below are relative to it. Adapter and install paths are of this repo at `fcf064f`.
- **Read** = source lines cited. **Ran** = a throwaway extension under `pi --no-extensions --no-skills --no-context-files --no-session --offline -e <file>`, in a scratch directory, in a private tmux server (`tmux -L rr-a1`, private `TMUX_TMPDIR`), with `PI_CODING_AGENT_DIR` pointing at an empty scratch directory (so no User settings, auth or session was read or written). A fake provider (`pi.registerProvider` with a `streamSimple` that waits 12 s) gave a turn that streams without a network. The probes sent keys to the private tmux only to put text in the editor or open a dialog for the test; the reload itself was always triggered by the extension.
- Not run: the real adapter, a real Lead with its packages (`lead_pi_packages`), plannotator, `ask_user_question`.

## Answer in one paragraph

**Yes, in-process and without typing:** an extension can call `pi.sendUserMessage("/<its-own-command>", { expandPromptTemplates: true })`. pi then runs the extension command's handler at once with a full `ExtensionCommandContext`, and the handler calls `await ctx.reload()`. Ran: pi reloaded, the new extension code (including a transitive module) was loaded, `session_shutdown{reason:"reload"}` then `session_start{reason:"reload"}` fired. Without `expandPromptTemplates: true` the text goes to the model as a prompt instead, and pi's built-in `/reload` can **not** be reached this way (it goes to the model too). The reload is **not safe by itself**: it ignores dialogs (it closes them and the awaiting promise never settles), it silently does nothing mid-turn, and pi's own `isIdle()` does not see dialogs. The adapter has to guard (idle + nothing blocked + nothing typed) before and inside the command.

## Where a command context comes from

| Fact | Source |
|---|---|
| `reload()` exists only on `ExtensionCommandContext`; the doc says these operations are command-only because calling them from lifecycle handlers can deadlock | `dist/core/extensions/types.d.ts:322` (`reload(): Promise<void>` in `ExtensionCommandContext`, `:286`), `docs/extensions.md:217-218` |
| It is added only by `createCommandContext()`; the context given to event handlers and tools (`createContext()`) has no `reload` | `dist/core/extensions/runner.js:724-756` (`context.reload = …` at `:753`); **ran:** `typeof ctx.reload` in a `session_start` handler is `undefined` |
| The only caller of `createCommandContext()` for a command is `_tryExecuteExtensionCommand`; the other is `createReplacedSessionContext` (after `newSession`/`fork`/`switchSession`, which replace the session: not wanted) | `dist/core/agent-session.js:1677`, `:3588` |
| `reload()` calls the mode's handler; interactive mode's is `handleReloadCommand()` | `dist/core/extensions/runner.js:753-756`, `dist/modes/interactive/interactive-mode.js:1528-1530` |
| After `await ctx.reload()` the old context is stale (`assertActive` throws); the handler must not use it | `dist/core/extensions/runner.js:482`, `docs/extensions.md:50` |

## Candidate mechanisms

### M1. `pi.sendUserMessage("/cmd", { expandPromptTemplates: true })` → the adapter's own command → `ctx.reload()` — **works**

- `pi.sendUserMessage` → `AgentSession.sendUserMessage` → `prompt(text, { expandPromptTemplates: options?.expandPromptTemplates ?? false, … })`: `dist/core/agent-session.js:2744-2751`, `:1880-1907`. The type documents the flag: "Set expandPromptTemplates to dispatch extension commands and expand skill commands and prompt templates" (`dist/core/extensions/types.d.ts:1262-1270`).
- In `prompt()`, when `expandPromptTemplates && text.startsWith("/")`, `_tryExecuteExtensionCommand(text)` runs **first** and, if the name is a registered extension command, calls `command.handler(args, createCommandContext())` and returns: `dist/core/agent-session.js:1606-1618`, `:1668-1690`. This is before the streaming check (`:1639`), so the handler runs **even while a turn is streaming** (the comment at `:1608` says so).
- **Ran** (`pi.registerCommand("cpd-reload-probe", { handler: async (_, ctx) => { await ctx.reload(); } })` and a 300 ms poll of a trigger file that calls `pi.sendUserMessage("/cpd-reload-probe", { expandPromptTemplates: true })`; log lines trimmed):
  ```
  cmd: isIdle=true editor=""
  session_shutdown reason=reload
  load v2 …                      # the file was edited v1 → v2 before the trigger
  session_start reason=reload v2
  cmd: ctx.reload() resolved
  ```
  The pane showed "Reloaded keybindings, extensions, skills, prompts, themes, and context files". The extension is re-imported with `moduleCache: false` (`dist/core/extensions/loader.js:477-481`), so a changed file is read again; **ran** with the real shape too: an entry file importing `./lib/ver.ts`, whole directory replaced as `scripts/install-cpd.sh` does (below): `load v2`, `session_start reload v2`.
- The command is visible to the User in autocomplete (`/cpd-…`). Choose a name that cannot be typed by accident (e.g. `cpd-reload-after-install`); the handler re-checks the guards, so a User typing it is harmless.

### M2. `pi.sendUserMessage("/cmd")` without `expandPromptTemplates` — **does not work**

The default is `false` (`dist/core/agent-session.js:1903`), so `prompt()` skips the command path and the text is sent to the model as a user message. **Ran:** `agent_start` fired, the transcript showed `/cpd-reload-probe` as a user message, the (fake) model answered; no reload.

### M3. `pi.sendUserMessage("/reload", { expandPromptTemplates: true })` — **does not work, and costs a turn**

`/reload` is not an extension command. It is matched only in the editor's submit handler of interactive mode (`dist/modes/interactive/interactive-mode.js:2656-2660`), never in `prompt()`. `getCommand("reload")` finds no extension command (`dist/core/extensions/runner.js:595-597`), so the text goes to the model. **Ran:** `agent_start`, transcript showed `/reload` as a user message; no reload.

### M4. `pi.sendMessage(...)` (custom message) — **does not work**

`sendCustomMessage` only appends a custom message and optionally starts a turn (`dist/core/agent-session.js:1814-1870`); it never calls `prompt()` and no command is dispatched. Read only.

### M5. Signals — **no reload signal exists**

Interactive mode registers `SIGTERM` and `SIGHUP` only, and both run `shutdown({ fromSignal: true })`, which ends the process (`dist/modes/interactive/interactive-mode.js:3577-3590`, `:3483-3500`). That is what the wrapper's reload (contract §6.7 "Reload on install": `SIGTERM` + `exec`) uses. Nothing in pi turns a signal into `reload()`. Read only.

### M6. The event bus, RPC, print mode — **no**

- `pi.events` is a plain `EventEmitter` between extensions (`dist/core/event-bus.js`); no channel of pi's own triggers a reload.
- `reload` appears in `rpc-mode.js:252-254` and `print-mode.js:74-76` only as the binding behind `ctx.reload()` for command handlers of those modes; the RPC command set has no reload request, and Leads run the TUI (`ctx.mode === "tui"`, `pi/lib/adapter.ts:start`). Read only.

### M7. Fake the keystrokes in-process — **not viable, and excluded by hs-d141**

`ctx.ui.setEditorText` / `pasteToEditor` fill the editor (`types.d.ts:133-137`); there is no API to submit it, and `onTerminalInput` only listens (`types.d.ts:82`). Even a hack would be typing into the pane. Not pursued.

### M8. Keep the wrapper's `SIGTERM` + `exec` (contract §6.7) — works today, separate decision

Not an in-process reload; it restarts pi and resumes the session file. Both can coexist: the in-process reload covers the parked and chat-slot cases cheaply, the wrapper remains the fallback when pi does not reload (see "What to verify").

## Is it safe? (hs-d49: only idle, never with unsent typing, never while blocked)

What `handleReloadCommand` and `session.reload` do, and what **ran**:

| Situation | What happens | Source / ran |
|---|---|---|
| **Mid-turn** (streaming or compacting) | The command handler still runs (M1). `ctx.isIdle()` is `false`. `handleReloadCommand` shows "Wait for the current response to finish before reloading." and returns; **`ctx.reload()` still resolves normally**, so the handler cannot tell it did not reload. | `interactive-mode.js:5363-5371`; `agent-session.js:1057-1059` (`isIdle` = no run and no compaction). **Ran:** `cmd: isIdle=false`, `cmd: ctx.reload() resolved`, no `session_shutdown`, the warning on screen, the turn went on. So the handler **must check `ctx.isIdle()` first**. |
| **A dialog is open** (`ctx.ui.select/confirm/input/editor`, an overlay, from any extension) | `pi` reports `isIdle() === true` (dialogs are not part of it). `reload` calls `resetExtensionUI()`, which hides the selector, input and editor and the overlay. The awaited `select()` promise is **never resolved** (`hideExtensionSelector` does not resolve it): the answer is lost and the awaiting handler hangs. | `interactive-mode.js:5372` (`resetExtensionUI()`), `:1898-1925`; `:2141-2149`. **Ran:** with `/cpd-dialog` open, `cmd: isIdle=true`, reload happened, dialog gone, and `dialog resolved` never appeared in the log. **Unsafe**; pi offers no flag the adapter can read (the "blocked" bookkeeping inside pi, `programStatus.setBlocked("extension-dialog")`, `interactive-mode.js:2134`, is only reported to the terminal as OSC 7501, not exposed to extensions). The adapter's own knowledge is the only guard: its `Facts` (own blocking reasons: escalations, `plan-review:*`) and the `herdr:blocked` signals of other extensions (`pi/lib/state.ts`, `adapter.ts` step 2) — the same set that makes the Session `blocked`. |
| **Unsent typing** in the editor | Kept. `ctx.ui.getEditorText()` is readable from the command. **Ran:** typed `unsent typing`, reload ran, the text was still in the editor afterwards. (Source: `setCustomEditorComponent` copies the text across editors, `interactive-mode.js:2242-2260`; other extensions' custom editors were not tested.) Because hs-d49 says never with unsent typing and a custom editor of a Lead package could behave differently, keep the guard `getEditorText() === ""`; it is cheap and a Lead decision can relax it. |
| **A queued/steered message, a pending turn** | `isIdle()` covers a run and compaction only; a retry delay or a continuation queued between runs is not separately visible (unverified for retry). Add `ctx.hasPendingMessages()` to the guard (it exists on `ExtensionContext`, `types.d.ts:244`; its semantics were not read or run). | `agent-session.js:1057-1059` |
| `session_shutdown` / `session_start` during reload | The adapter's `stop()` reports `stopped` (waiting at most 2 s) and `start()` reports `idle` again and restarts the follower from the entries' cursor (`adapter.ts:210-221`, `:96-206`). `stopped → idle` is the normal start-up transition. Not run with the real adapter. | `types.d.ts:636-639` (`session_shutdown`, `reason: "reload"`), `agent-session.js:2968-3002` |
| The reload fails (extension throws on load, install half done) | `handleReloadCommand` catches it and shows "Reload failed: …". The old runtime has already been invalidated (`oldRunner.invalidate()`, `agent-session.js:2972`), so pi may be left without the adapter until a later `/reload` or restart. | `interactive-mode.js:5432-5437`; **not run** |

**Guard, in this order, both at the trigger and again inside the command** (the command is authoritative: only it has `ctx`): `ctx.isIdle()`; adapter `Facts.wanted()` is `idle`; `ctx.ui.getEditorText() === ""`; no open Plan review/escalation. If a guard fails the command does nothing and the detector keeps polling; it must **not** mark the install as handled.

## How the adapter detects that its installed package changed

- Where the adapter is: `import.meta.url` works inside an extension loaded by pi's jiti (**ran:** `dirname(fileURLToPath(import.meta.url))` gave the real directory). `dirname` of `pi/lib/` is the adapter directory (`~/.local/share/cpd/pi`, contract §8 `pi_adapter`); that is the path pi was given with `-e` (`core/src/host/pi.rs:lead_candidates`).
- What an install does: `scripts/install-cpd.sh` replaces the **binary first**, then the adapter as a whole: `cp -R pi/. <dir>.tmp.$$`, `rm -rf <dir>`, `mv <dir>.tmp.$$ <dir>` (`scripts/install-cpd.sh`, the "adapter is replaced as a whole" block). So the directory gets a **new inode and mtime**, and for a short moment **does not exist**.
- Detection (same idea as `core/src/host/exe.rs`, hs-d33): at load record `(dev, ino, mtimeMs)` of the adapter directory via `statSync`; poll it (**ran** at 200 ms; a few seconds is enough in production; `timer.unref()` so it never keeps pi alive; clear it in `session_shutdown`). Different identity ⇒ new install. `ENOENT` is **not** a change: it is the `rm`→`mv` window, wait. **Ran** against the same `rm -rf; mv` sequence: four `stat: missing` polls, then `changed <old> -> <new>`, reload, `load v2` (including `lib/ver.ts`), `session_start reload v2`. Because `mv` is a rename of a finished directory, the first poll that sees the directory sees the complete new package, skills included (`pi/package.json` `pi.skills`).
- Do not key on the `cpd` binary: it changes first, before the adapter directory is complete; a reload then would load the old adapter again.
- A poll (not `fs.watch`) is deliberate: no watcher to tear down (teardown audit), and one `stat` per few seconds.

## Recommendation

1. Add to the adapter one command (e.g. `cpd-reload-after-install`) whose handler is: guard (above) → `await ctx.reload()`; nothing after it uses the old `ctx`.
2. At `session_start` record the adapter directory's identity and start an unref'd poll. On a changed, existing directory and while its own guards pass, call `pi.sendUserMessage("/cpd-reload-after-install", { expandPromptTemplates: true })`. If the command refuses, keep polling and try again at the next tick; after a successful reload the new instance records the new identity.
3. Deliver nothing to the model: do not use `sendMessage`/`sendUserMessage` without the flag (M2, M3 start a turn).
4. Keep the wrapper's `SIGTERM` + `exec` (§6.7) as the fallback only for what the in-process path cannot do (no `tui` mode, a refused reload for a long time, or an adapter that cannot start). The contract text then needs the amendment before the code (hs-d4): the wrapper's idle/parked rules and this guard are two implementations of hs-d49.
5. The `herdr:blocked` coverage of dialogs decides how safe step 2 is (see below); until a Lead's dialog extensions are verified to emit it, an in-process reload can kill their dialog.

## What to verify by a live test (not done here)

1. **`ask_user_question` (and every Lead package) emits `herdr:blocked`** around its dialog, so `Facts` sees it (`docs/commands.md` §15.4 already marks this "unverified for every Lead package"). Open its dialog in a real Lead, trigger the reload, and check that the guard refuses.
2. The real adapter across a reload: `stopped` then `idle` reported, the follower restarted without a duplicate or lost Event, a pending `plan-review` restored from entries (`adapter.ts:start` step 5), plannotator and the `cpd_plan_review` tool re-registered (`registerTool`, `toolRegistered` is module-level state that is rebuilt by the new load).
3. A Lead with custom editors/footers/widgets from its packages: unsent typing is kept (ran only with the default editor), and `resetExtensionUI()` (`interactive-mode.js:1898-1925`) does not drop something the User needs.
4. Reload when the install is half done (adapter directory gone for longer than the poll, or a package whose `extension.ts` throws): the failure path (`interactive-mode.js:5432-5437`) leaves pi with no adapter; decide whether the guard should first `import()` the new entry to check it, or accept the wrapper's restart as recovery.
5. `ctx.hasPendingMessages()` and a retry in progress: whether `isIdle()` is true during a retry delay (pi's comment at `agent-session.js:1056` says it covers "retry or queued continuation", the code at `:1057-1059` does not).
6. Behaviour while the pane is **parked** (the window is not in the chat slot): the same code path runs (no terminal needed for `ctx.reload()`), but the TUI's render of the reload box was only ran in a real pane.
