import { createMcpExtension, type ExtensionFactory, type McpExtensionOptions, type McpServerConfig } from "@earendil-works/pi-coding-agent";
import { CpdError } from "./cpd.ts";
import type { PiApi } from "./pi.ts";

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
