import { WebError, toWireError } from "../../protocol/domain.ts";
import type { AssistantBlock, ToolItem, ToolResult } from "../../protocol/domain.ts";
import type { Disposable, Scheduler } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";

export const LIVE_RETRY_MS = 1000;

export interface LiveConnection {
  onLine(cb: (line: string) => void): void;
  onClose(cb: () => void): void;
  close(): void;
}

export interface LiveSource {
  /** `committed(ts)` tells whether an assistant `message.timestamp` is already in the transcript. */
  attach(sessionId: string, emit: (msg: SessionServerMessage) => void, committed: (timestamp: number) => boolean): void;
  detach(): void;
}

export interface LiveDeps {
  /** Throws when no socket listens. */
  connect(path: string): LiveConnection;
  scheduler: Scheduler;
  cpdHome: string;
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);

/** The text and thinking blocks of a message `content` (toolCall content is not a block). */
function blocksOf(content: unknown): AssistantBlock[] {
  const out: AssistantBlock[] = [];
  if (!Array.isArray(content)) return out;
  for (const b of content) {
    if (!isObj(b)) continue;
    if (b.type === "text" && typeof b.text === "string") out.push({ type: "text", text: b.text });
    else if (b.type === "thinking" && typeof b.thinking === "string") out.push({ type: "thinking", text: b.thinking });
  }
  return out;
}

/** Text blocks of a tool result `{content}` joined with "\n". */
function resultText(result: unknown): string {
  if (!isObj(result) || !Array.isArray(result.content)) return "";
  return result.content
    .filter((b): b is Obj => isObj(b) && b.type === "text" && typeof b.text === "string")
    .map((b) => b.text as string)
    .join("\n");
}

const timestampOf = (m: Obj): number | undefined => (typeof m.timestamp === "number" && Number.isFinite(m.timestamp) ? m.timestamp : undefined);

export function createLiveSource(deps: LiveDeps): LiveSource {
  let stopped = true;
  let conn: LiveConnection | null = null;
  let timer: Disposable | null = null;

  function stop(): void {
    stopped = true;
    timer?.dispose();
    timer = null;
    const c = conn;
    conn = null;
    c?.close();
  }

  function attach(sessionId: string, emit: (msg: SessionServerMessage) => void, committed: (timestamp: number) => boolean): void {
    stopped = false;
    const out = (m: SessionServerMessage): void => {
      if (!stopped) emit(m);
    };
    // The open assistant message, set by message_start or inflight. `blocks` maps a content index to its block index, allocated at its first emitted delta.
    let open: { ts: number; model: string | undefined; next: number; blocks: Map<number, number> } | null = null;
    const tools = new Map<string, { name: string; args: Record<string, unknown> }>();

    const toolItem = (callId: string, status: ToolItem["status"], result?: ToolResult): ToolItem | null => {
      const kept = tools.get(callId);
      if (kept === undefined) return null;
      return { kind: "tool", id: `tool:${callId}`, callId, name: kept.name, args: kept.args, status, ...(result === undefined ? {} : { result }) };
    };
    const startTool = (callId: string, name: string, args: unknown): void => {
      tools.set(callId, { name, args: isObj(args) ? args : {} });
      const item = toolItem(callId, "running");
      if (item !== null) out({ type: "session.append", sessionId, items: [item] });
    };
    const updateTool = (callId: string, status: ToolItem["status"], result: unknown, isError: boolean): void => {
      const item = toolItem(callId, status, { text: resultText(result), isError });
      if (item !== null) out({ type: "session.update", sessionId, items: [item] });
    };
    const openMessage = (m: Obj, withBlocks: boolean): void => {
      const ts = timestampOf(m);
      if (ts === undefined) return;
      const blocks = withBlocks ? blocksOf(m.content) : [];
      const model = typeof m.model === "string" ? m.model : undefined;
      const map = new Map<number, number>();
      if (withBlocks && Array.isArray(m.content)) {
        let n = 0;
        m.content.forEach((b, i) => {
          if (isObj(b) && ((b.type === "text" && typeof b.text === "string") || (b.type === "thinking" && typeof b.thinking === "string"))) map.set(i, n++);
        });
      }
      open = { ts, model, next: blocks.length, blocks: map };
      if (committed(ts)) return;
      out({ type: "session.append", sessionId, items: [{ kind: "assistant", id: `live:${ts}`, blocks, streaming: true, ...(model === undefined ? {} : { model }) }] });
    };

    const frame = (f: Obj): void => {
      switch (f.t) {
        case "hello":
          if (f.protocol !== 1) {
            out({ type: "session.error", sessionId, error: toWireError(new WebError("protocol_error", `unsupported pi live protocol ${String(f.protocol)}`, { field: "protocol" })) });
            stop();
          }
          return;
        case "inflight": {
          open = null;
          tools.clear();
          if (isObj(f.message) && f.message.role === "assistant") openMessage(f.message, true);
          if (Array.isArray(f.tools)) {
            for (const t of f.tools) {
              if (!isObj(t) || typeof t.toolCallId !== "string") continue;
              startTool(t.toolCallId, typeof t.toolName === "string" ? t.toolName : "", t.args);
              if (t.partialResult !== null && t.partialResult !== undefined) updateTool(t.toolCallId, "running", t.partialResult, false);
            }
          }
          return;
        }
        case "message_start":
          if (isObj(f.message) && f.message.role === "assistant") openMessage(f.message, false);
          return;
        case "delta": {
          if (open === null || committed(open.ts)) return;
          if ((f.kind !== "text" && f.kind !== "thinking") || typeof f.index !== "number") return;
          let blockIndex = open.blocks.get(f.index);
          if (blockIndex === undefined) {
            // A block takes its index only at its first emitted delta, so indexes stay consecutive with the client's blocks.
            if (f.phase !== "delta" || typeof f.delta !== "string") return;
            blockIndex = open.next++;
            open.blocks.set(f.index, blockIndex);
          }
          if (f.phase === "delta" && typeof f.delta === "string") {
            out({ type: "session.delta", sessionId, itemId: `live:${open.ts}`, blockIndex, blockType: f.kind, text: f.delta });
          }
          return;
        }
        case "message_end": {
          if (!isObj(f.message) || f.message.role !== "assistant") return;
          const ts = timestampOf(f.message);
          const kept = open !== null && open.ts === ts ? open.model : undefined;
          open = null;
          if (ts === undefined || committed(ts)) return;
          const blocks = blocksOf(f.message.content);
          if (blocks.length === 0) {
            out({ type: "session.remove", sessionId, itemId: `live:${ts}` });
            return;
          }
          const model = typeof f.message.model === "string" ? f.message.model : kept;
          out({ type: "session.update", sessionId, items: [{ kind: "assistant", id: `live:${ts}`, blocks, streaming: false, ...(model === undefined ? {} : { model }) }] });
          return;
        }
        case "tool_start":
          if (typeof f.toolCallId === "string") startTool(f.toolCallId, typeof f.toolName === "string" ? f.toolName : "", f.args);
          return;
        case "tool_update":
          if (typeof f.toolCallId === "string") updateTool(f.toolCallId, "running", f.partialResult, false);
          return;
        case "tool_end":
          if (typeof f.toolCallId === "string") updateTool(f.toolCallId, f.isError === true ? "failed" : "done", f.result, f.isError === true);
          return;
        default:
          return;
      }
    };

    const onLine = (line: string): void => {
      let value: unknown;
      try {
        value = JSON.parse(line);
      } catch {
        return;
      }
      if (isObj(value) && typeof value.t === "string") frame(value);
    };

    const retry = (): void => {
      if (stopped) return;
      timer = deps.scheduler.after(LIVE_RETRY_MS, () => {
        timer = null;
        connect();
      });
    };
    function connect(): void {
      if (stopped) return;
      open = null;
      tools.clear();
      try {
        const c = deps.connect(`${deps.cpdHome}/run/pi/${sessionId}.sock`);
        conn = c;
        c.onLine(onLine);
        c.onClose(() => {
          if (conn === c) conn = null;
          retry();
        });
      } catch {
        retry();
      }
    }
    connect();
  }

  return { attach, detach: stop };
}
