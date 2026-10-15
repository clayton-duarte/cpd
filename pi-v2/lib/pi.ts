/** The part of pi's extension API the adapter uses (docs: pi `extensions.md`). Structural, so tests pass a fake. */

export type ExecResult = { stdout: string; stderr: string; code: number };
export type Exec = (command: string, args: string[]) => Promise<ExecResult>;

export type Message = { customType: string; content: string; display: boolean; details: unknown };
export type SendOptions = { triggerTurn: boolean; deliverAs: "followUp" };

export type Ctx = {
  mode: string;
  ui: { notify(message: string, level?: string): void };
  sessionManager: { getSessionId(): string; getEntries(): readonly unknown[] };
  isIdle(): boolean;
};

export type ToolDefinition = {
  name: string;
  label: string;
  description: string;
  parameters: unknown;
  exposure?: string;
  execute(toolCallId: string, params: unknown, signal: AbortSignal | undefined, onUpdate: unknown, ctx: Ctx): Promise<{ content: { type: "text"; text: string }[]; details: unknown }>;
};

export type PiApi = {
  on(event: string, handler: (event: any, ctx: Ctx) => unknown): unknown;
  events: { emit(channel: string, data: unknown): void; on(channel: string, handler: (data: unknown) => void): () => void };
  exec: Exec;
  sendMessage(message: Message, options: SendOptions): void;
  appendEntry(customType: string, data: unknown): void;
  setSessionName(name: string): void;
  getSessionName(): string | undefined;
  registerTool(tool: ToolDefinition): void;
};
