import { createMcpExtension, type ExtensionFactory, type McpExtensionOptions, type McpServerConfig } from "@earendil-works/pi-coding-agent";
import { CpdError } from "./cpd.ts";
import { parseMcpEvents } from "./event.ts";
import type { Host } from "./host.ts";
import type { PiApi } from "./pi.ts";
import type { Run } from "./run.ts";

/** contract §6.11: pi's own MCP extension with no file config: no server from `mcp.json`, none from a project. */
export function mcpMount(overrides?: McpExtensionOptions): ExtensionFactory {
  return createMcpExtension({ ...overrides, loadConfig: () => ({ servers: [], errors: [] }) });
}

/** contract §6.11, commands.md §15.3 `mcp-attached`: registers the server with pi. Throws `mcp_register_failed` when pi refuses; the caller delivers it. */
export function attachServer(pi: Pick<PiApi, "registerMcpServer">, name: string, server: McpServerConfig): void {
  try {
    pi.registerMcpServer(name, server);
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    throw new CpdError("mcp_register_failed", message, { name, message });
  }
}

/** The page size of the replay read; a full page may be missing the latest Event of a name. */
const REPLAY_LIMIT = 5000;

/** commands.md §15.2 step 5a: registers the latest own `mcp-attached` Event of each name, in id order; every Event read counts as handled for this Run. */
export async function replayServers(host: Host, r: Run): Promise<void> {
  const what = "events list (mcp-attached)";
  let events;
  try {
    const out = await host.call(["events", "list", "--session", r.id, "--kind", "mcp-attached", "--limit", String(REPLAY_LIMIT)]);
    if (host.run !== r) return;
    events = parseMcpEvents(out);
    if (events.length >= REPLAY_LIMIT) throw new CpdError("mcp_replay_truncated", `the first ${REPLAY_LIMIT} mcp-attached Events fill a page: the latest of a name may be missing`);
  } catch (e) {
    if (host.run === r) host.deliver(what, e);
    return;
  }
  const own = events.filter((e) => e.session === r.id);
  const latest = new Map<string, (typeof own)[number]>();
  for (const e of own) {
    r.replayed.add(e.id);
    const seen = latest.get(e.data.name);
    if (seen === undefined || e.id > seen.id) latest.set(e.data.name, e);
  }
  for (const e of [...latest.values()].sort((a, b) => a.id - b.id)) {
    try {
      attachServer(host.pi, e.data.name, e.data.server as unknown as McpServerConfig);
    } catch (err) {
      host.deliver("mcp attach", err, { name: e.data.name });
    }
  }
}
