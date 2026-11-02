import { defineExtension, type Extension } from "@earendil-works/pi-durable";
import type { TSchema } from "typebox";
import { CpdError } from "../lib/cpd.ts";
import { parseMcpEvents, type EventOf } from "../lib/event.ts";
import type { CodemodeServer } from "../durable-host/codemode.ts";
import type { Input } from "./deliver.ts";

export type McpConnection = {
  tools: readonly { tool: string; name: string; description: string; inputSchema: Record<string, unknown> }[];
  call(tool: string, args: Record<string, unknown>): Promise<{ text: string; isError: boolean }>;
  close(): Promise<void>;
};

export type McpPort = { connect(name: string, server: Record<string, unknown>): Promise<McpConnection> };

export type McpHost = {
  session: string;
  port: McpPort;
  install(extension: Extension): void;
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  submit(input: Input): Promise<void>;
  log(line: string): void;
  attached(servers: readonly CodemodeServer[]): void;
  over(): boolean;
};

const REPLAY_LIMIT = 5000;
const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e)).slice(0, 2000) || "unknown error";

const extensionOf = (name: string, connection: McpConnection): Extension =>
  defineExtension({
    name: `mcp-${name}`,
    tools: connection.tools.map((entry) => ({
      name: entry.name,
      description: entry.description,
      parameters: entry.inputSchema as unknown as TSchema,
      async execute(args: unknown) {
        const r = await connection.call(entry.tool, args as Record<string, unknown>);
        return { output: [{ type: "text" as const, text: r.text }], isError: r.isError };
      },
    })),
  } as Parameters<typeof defineExtension>[0]);

/** Contract §6.12 "MCP servers in the durable runner": one Event at a time, never throws, one open connection per name. */
export function mcpHandler(host: McpHost) {
  const open = new Map<string, McpConnection>();
  const handled = new Set<number>();
  let chain: Promise<void> = Promise.resolve();

  const reportFailure = async (event: EventOf<"mcp-attached">, name: string, e: unknown): Promise<void> => {
    const message = messageOf(e);
    try {
      await host.cpd(["session", "report", "--what", `mcp attach ${name}`, "--code", "mcp_register_failed", "--message", message, "--details", JSON.stringify({ name, message }), "--session", host.session]);
    } catch (err) {
      host.log(`report failed: ${err instanceof CpdError ? err.code : "error"}: ${messageOf(err)}`);
    }
    await host.submit({
      content: `cpd adapter: mcp attach ${name} failed: ${message}. Sign in with 'pi mcp login ${name}' if it needs sign-in, then run cpd mcp attach ${name} again.`,
      requestId: `mcp-failed:${event.id}`,
      whenBusy: "followUp",
    });
  };

  const attach = async (event: EventOf<"mcp-attached">): Promise<void> => {
    if (host.over()) return;
    const name = event.data.name;
    let connection: McpConnection;
    try {
      connection = await host.port.connect(name, event.data.server);
    } catch (e) {
      await reportFailure(event, name, e);
      return;
    }
    if (host.over()) {
      await connection.close().catch((e) => host.log(`mcp close: ${messageOf(e)}`));
      return;
    }
    const previous = open.get(name);
    host.install(extensionOf(name, connection));
    open.set(name, connection);
    await previous?.close().catch((e) => host.log(`mcp close: ${messageOf(e)}`));
  };

  const enqueue = (event: EventOf<"mcp-attached">): void => {
    chain = chain.then(() => attach(event)).catch((e) => host.log(`mcp attach ${event.data.name}: ${messageOf(e)}`));
  };

  return {
    handle(event: EventOf<"mcp-attached">): void {
      if (event.session !== host.session || handled.has(event.id)) return;
      handled.add(event.id);
      enqueue(event);
    },
    async replay(): Promise<void> {
      let events: EventOf<"mcp-attached">[];
      try {
        const out = await host.cpd(["events", "list", "--session", host.session, "--kind", "mcp-attached", "--limit", String(REPLAY_LIMIT)]);
        events = parseMcpEvents(out);
        if (events.length >= REPLAY_LIMIT) throw new Error(`the first ${REPLAY_LIMIT} mcp-attached Events fill a page: the latest of a name may be missing`);
      } catch (e) {
        host.log(`mcp replay: ${messageOf(e)}`);
        return;
      }
      const latest = new Map<string, EventOf<"mcp-attached">>();
      for (const e of events.filter((x) => x.session === host.session)) {
        handled.add(e.id);
        const seen = latest.get(e.data.name);
        if (seen === undefined || e.id > seen.id) latest.set(e.data.name, e);
      }
      for (const e of [...latest.values()].sort((a, b) => a.id - b.id)) enqueue(e);
    },
    /** Waits for the running handler work, then closes every open connection once. */
    async closeAll(): Promise<void> {
      await chain;
      const all = [...open.values()];
      open.clear();
      for (const c of all) await c.close().catch((e) => host.log(`mcp close: ${messageOf(e)}`));
    },
  };
}
