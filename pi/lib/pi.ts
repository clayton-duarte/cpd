/** The part of pi's extension API the adapter uses (docs: pi `extensions.md`). Structural, so tests pass a fake. */

/** pi's `ExecOptions` (`core/exec.d.ts`): `timeout` in ms; both end the process with SIGTERM. */
export type ExecOptions = { signal?: AbortSignal; timeout?: number };
/** pi's `ExecResult`: `killed` is set when the timeout or the signal ended the process (its `code` is then not an exit code). */
export type ExecResult = { stdout: string; stderr: string; code: number; killed: boolean };
export type Exec = (command: string, args: string[], options?: ExecOptions) => Promise<ExecResult>;

import type { Component } from "@earendil-works/pi-tui";

export type Message = { customType: string; content: string; display: boolean; details: unknown };
export type SendOptions = { triggerTurn: boolean; deliverAs: "followUp" | "steer" };

export type Ctx = {
  mode: string;
  cwd: string;
  isProjectTrusted(): boolean;
  ui: { notify(message: string, level?: string): void; getEditorText(): string };
  sessionManager: { getSessionId(): string; getEntries(): readonly unknown[] };
  isIdle(): boolean;
  hasPendingMessages(): boolean;
};

/** The context of an extension command: the only one with `reload()`. It is stale once `reload()` has resolved. */
export type CommandCtx = Ctx & { reload(): Promise<void> };

export type ToolDefinition = {
  name: string;
  label: string;
  description: string;
  parameters: unknown;
  exposure?: string;
  execute(toolCallId: string, params: unknown, signal: AbortSignal | undefined, onUpdate: unknown, ctx: Ctx): Promise<{ content: { type: "text"; text: string }[]; details: unknown }>;
};

/** A custom message as pi hands it to a renderer, reduced to what the adapter reads. */
export type RenderedMessage = { customType: string; content: unknown; details?: unknown };
/** pi's `Theme`, reduced to what the renderer paints with (colour names are pi's `ThemeColor` and `ThemeBg`). */
export type Theme = { fg(color: string, text: string): string; bg(color: string, text: string): string; bold(text: string): string };
export type MessageRenderer = (message: RenderedMessage, options: { expanded: boolean; outputPad: number }, theme: Theme) => Component | undefined;

export type PiApi = {
  on(event: string, handler: (event: any, ctx: Ctx) => unknown): unknown;
  events: { emit(channel: string, data: unknown): void; on(channel: string, handler: (data: unknown) => void): () => void };
  exec: Exec;
  sendMessage(message: Message, options: SendOptions): void;
  appendEntry(customType: string, data: unknown): void;
  setSessionName(name: string): void;
  getSessionName(): string | undefined;
  registerMessageRenderer(customType: string, renderer: MessageRenderer): void;
  registerTool(tool: ToolDefinition): void;
  registerCommand(name: string, options: { description?: string; handler: (args: string, ctx: CommandCtx) => Promise<void> }): void;
  /** With `expandPromptTemplates` an extension command runs at once; without it the text goes to the model. */
  sendUserMessage(content: string, options: { expandPromptTemplates: boolean }): void;
};
