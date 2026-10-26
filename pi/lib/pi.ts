/** The part of pi's extension API the adapter uses: pi's own types (docs: pi `extensions.md`), narrowed to the members the adapter calls so a test can pass a fake. Types only: nothing of pi is bundled or run. */
import type { ExtensionAPI, ExtensionCommandContext, ExtensionContext, ToolDefinition } from "@earendil-works/pi-coding-agent";
import type { Component } from "@earendil-works/pi-tui";

export type Ctx = Pick<ExtensionContext, "mode" | "isIdle" | "hasPendingMessages" | "cwd" | "isProjectTrusted"> & {
  ui: Pick<ExtensionContext["ui"], "notify" | "setStatus" | "getEditorText" | "select" | "editor">;
  sessionManager: Pick<ExtensionContext["sessionManager"], "getSessionId" | "getEntries" | "getBranch">;
};

/** The context handed to `session_start`: `Ctx` and pi's model registry, which the model switch reads (commands.md §15.3 `session-retiered`). */
export type ModelCtx = Ctx & Pick<ExtensionContext, "modelRegistry">;

/** The context of an extension command: the only one with `reload()`. It is stale once `reload()` has resolved. */
export type CommandCtx = Ctx & Pick<ExtensionCommandContext, "reload">;

export type PiApi = Pick<
  ExtensionAPI,
  "on" | "events" | "exec" | "sendMessage" | "appendEntry" | "setSessionName" | "getSessionName" | "registerMessageRenderer" | "registerTool" | "registerCommand" | "sendUserMessage" | "setModel" | "setThinkingLevel" | "registerMcpServer"
>;

export type Exec = PiApi["exec"];
/** pi's `ExecOptions` (`core/exec.d.ts`): `timeout` in ms; `timeout` and `signal` both end the process with SIGTERM. */
export type ExecOptions = NonNullable<Parameters<Exec>[2]>;
/** pi's `ExecResult`: `killed` is set when the timeout or the signal ended the process (its `code` is then not an exit code). */
export type ExecResult = Awaited<ReturnType<Exec>>;
/** What the adapter sends: always a string `content`. */
export type Message = Parameters<PiApi["sendMessage"]>[0] & { content: string };
export type SendOptions = NonNullable<Parameters<PiApi["sendMessage"]>[1]>;
export type { ToolDefinition };

/** A custom message as pi hands it to a renderer, reduced to what the adapter reads. `timestamp` is pi's `CustomMessage.timestamp` (Unix ms), the source of the fold's local time. */
export type RenderedMessage = { customType: string; content: unknown; details?: unknown; timestamp: number };
/** pi's `Theme`, reduced to what the renderer paints with (colour names are pi's `ThemeColor` and `ThemeBg`). */
export type Theme = { fg(color: string, text: string): string; bg(color: string, text: string): string; bold(text: string): string };
/** The adapter's renderers take the narrow types above, which pi's own `MessageRenderer` arguments satisfy. */
export type MessageRenderer = (message: RenderedMessage, options: { expanded: boolean; outputPad: number }, theme: Theme) => Component | undefined;
