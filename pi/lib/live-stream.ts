// The pi live-stream socket of a tmux pi Session (docs/contract.md §6.21). 
import type { Duplex } from "node:stream";

export interface LiveStreamDeps {
  /** Listens at `path`; throws when it cannot. `onConnection` gets each client as a duplex stream. */
  listen(path: string, onConnection: (connection: Duplex) => void): { close(): void };
  mkdir(path: string, mode: number): void;
  unlink(path: string): void;
  home: string;
  session: string;
  stderr(text: string): void;
}

export interface LiveStreamPi {
  on(event: string, handler: (event: any, ctx?: unknown) => unknown): unknown;
}

export interface LiveStream {
  attach(pi: LiveStreamPi): void;
  dispose(): void;
}

const MAX_PENDING = 1024 * 1024;
const KINDS = new Set(["thinking", "text", "toolcall"]);
const PHASES: Record<string, string> = { start: "start", delta: "delta", end: "end" };

type Tool = { toolCallId: string; toolName: string; args: unknown; partialResult: unknown };

export function createLiveStream(deps: LiveStreamDeps): LiveStream {
  const path = `${deps.home}/run/pi/${deps.session}.sock`;
  const clients = new Set<Duplex>();
  let server: { close(): void } | undefined;
  let active = false;
  let message: unknown = null;
  const tools = new Map<string, Tool>();

  const drop = (c: Duplex) => {
    clients.delete(c);
    try {
      c.destroy();
    } catch {
      // a client that cannot be destroyed is already gone
    }
  };
  const writeTo = (c: Duplex, line: string) => {
    try {
      c.write(line);
      if (c.writableLength > MAX_PENDING) drop(c);
    } catch {
      drop(c);
    }
  };
  const send = (frame: object) => {
    if (!active || clients.size === 0) return;
    const line = `${JSON.stringify(frame)}\n`;
    for (const c of [...clients]) writeTo(c, line);
  };

  const close = () => {
    active = false;
    for (const c of [...clients]) drop(c);
    const s = server;
    server = undefined;
    message = null;
    tools.clear();
    if (s === undefined) return;
    try {
      s.close();
    } catch {
      // closing must not throw into pi
    }
    try {
      deps.unlink(path);
    } catch {
      // the file may already be gone
    }
  };

  const open = () => {
    close();
    try {
      deps.mkdir(`${deps.home}/run/pi`, 0o700);
      deps.unlink(path);
      server = deps.listen(path, (c) => {
        if (!active) return drop(c);
        c.on("error", () => drop(c));
        c.on("close", () => clients.delete(c));
        c.on("data", () => {}); // client input is ignored
        clients.add(c);
        writeTo(c, `${JSON.stringify({ t: "hello", protocol: 1, session: deps.session })}\n`);
        if (clients.has(c)) writeTo(c, `${JSON.stringify({ t: "inflight", message, tools: [...tools.values()] })}\n`);
      });
      active = true;
    } catch (e) {
      deps.stderr(`pi live-stream socket: ${e instanceof Error ? e.message : String(e)}\n`);
    }
  };

  const guard = <E>(f: (e: E) => void) => (e: E) => {
    try {
      f(e);
    } catch {
      // the stream must never throw into pi
    }
  };

  return {
    attach(pi) {
      pi.on("session_start", guard(() => open()));
      pi.on("session_shutdown", guard(() => close()));
      pi.on("agent_start", guard(() => send({ t: "agent_start" })));
      pi.on("agent_end", guard(() => send({ t: "agent_end" })));
      pi.on("message_start", guard((e: any) => {
        if (e.message?.role === "assistant") message = e.message;
        send({ t: "message_start", message: e.message });
      }));
      pi.on("message_end", guard((e: any) => {
        if (e.message?.role === "assistant") message = null;
        send({ t: "message_end", message: e.message });
      }));
      pi.on("message_update", guard((e: any) => {
        if (!active) return;
        const ev = e.assistantMessageEvent;
        if (e.message !== undefined) message = e.message;
        const type: unknown = ev?.type;
        if (typeof type !== "string") return;
        const cut = type.indexOf("_");
        const kind = type.slice(0, cut);
        const phase = PHASES[type.slice(cut + 1)];
        if (cut < 0 || !KINDS.has(kind) || phase === undefined) return;
        const frame: Record<string, unknown> = { t: "delta", kind, phase, index: ev.contentIndex };
        if (phase === "delta") frame.delta = ev.delta;
        if (phase === "end" && kind === "toolcall") frame.toolCall = ev.toolCall;
        send(frame);
      }));
      pi.on("tool_execution_start", guard((e: any) => {
        if (active) tools.set(e.toolCallId, { toolCallId: e.toolCallId, toolName: e.toolName, args: e.args, partialResult: null });
        send({ t: "tool_start", toolCallId: e.toolCallId, toolName: e.toolName, args: e.args });
      }));
      pi.on("tool_execution_update", guard((e: any) => {
        const tool = tools.get(e.toolCallId);
        if (tool) tool.partialResult = e.partialResult;
        send({ t: "tool_update", toolCallId: e.toolCallId, partialResult: e.partialResult });
      }));
      pi.on("tool_execution_end", guard((e: any) => {
        tools.delete(e.toolCallId);
        send({ t: "tool_end", toolCallId: e.toolCallId, result: e.result, isError: e.isError });
      }));
    },
    dispose: close,
  };
}
