import type { ExtensionFactory, McpExtensionOptions, McpServerConfig } from "@earendil-works/pi-coding-agent";
import type { PiApi } from "./pi.ts";

/** contract §6.10: pi's own MCP extension with no file config. STUB: does nothing yet. */
export function mcpMount(_overrides?: McpExtensionOptions): ExtensionFactory {
  return () => {};
}

/** contract §6.10, commands.md §15.3 `mcp-attached`: registers the server with pi. STUB: does nothing yet. */
export function attachServer(_pi: Pick<PiApi, "registerMcpServer">, _name: string, _server: McpServerConfig): void {}
