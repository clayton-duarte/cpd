/** The part of pi's extension API the adapter uses: pi's own types (docs: pi `extensions.md`), narrowed to the members the adapter calls so a test can pass a fake. Types only: nothing of pi is bundled or run. */
import type { ExtensionAPI, ExtensionCommandContext, ExtensionContext, ToolDefinition } from "@earendil-works/pi-coding-agent";

export type Ctx = Pick<ExtensionContext, "mode" | "isIdle" | "hasPendingMessages"> & {
  ui: Pick<ExtensionContext["ui"], "notify" | "getEditorText">;
  sessionManager: Pick<ExtensionContext["sessionManager"], "getSessionId" | "getEntries">;
};

/** The context of an extension command: the only one with `reload()`. It is stale once `reload()` has resolved. */
export type CommandCtx = Ctx & Pick<ExtensionCommandContext, "reload">;

export type PiApi = Pick<
  ExtensionAPI,
  "on" | "events" | "exec" | "sendMessage" | "appendEntry" | "setSessionName" | "getSessionName" | "registerTool" | "registerCommand" | "sendUserMessage"
>;

export type Exec = PiApi["exec"];
/** What the adapter sends: always a string `content`. */
export type Message = Parameters<PiApi["sendMessage"]>[0] & { content: string };
export type SendOptions = NonNullable<Parameters<PiApi["sendMessage"]>[1]>;
export type { ToolDefinition };
