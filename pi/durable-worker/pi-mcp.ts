import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";
import { loadPi, resultText } from "../lib/slack-channel.ts";
import type { McpPort } from "./mcp.ts";

/** pi's `createMcpToolName`, by file URL (pi's package.json exports none of its modules). */
async function loadToolName(codingAgentEntry: string): Promise<(server: string, tool: string, isTaken: (n: string) => boolean) => string> {
  const file = "extensions/mcp/tools.js";
  let module: Record<string, unknown>;
  try {
    module = await import(pathToFileURL(join(dirname(codingAgentEntry), file)).href);
  } catch (e) {
    throw new Error(`Cannot load pi's ${file}: ${(e as Error).message}`);
  }
  if (typeof module.createMcpToolName !== "function") throw new Error(`${file} does not export createMcpToolName`);
  return module.createMcpToolName as never;
}

/** The durable runner's McpPort over pi's own MCP client. */
export function piMcpPort(codingAgentEntry: string, cwd: string): McpPort {
  return {
    async connect(name, server) {
      const pi = await loadPi(codingAgentEntry);
      const createMcpToolName = await loadToolName(codingAgentEntry);
      const connection = new pi.McpServerConnection({
        entry: { name, config: server, source: "extension" },
        cwd,
        createTransport: pi.createDefaultTransport,
        credentials: new pi.McpOAuthCredentialStore(),
        onTools: () => {},
      });
      const needsSignIn = () => new Error(`${name} needs a sign-in: run pi mcp login ${name}`);
      try {
        await connection.getClient();
        if (connection.state === "needs-auth") throw needsSignIn();
      } catch (e) {
        await connection.close();
        throw connection.state === "needs-auth" ? needsSignIn() : e;
      }
      const taken = new Set<string>();
      const tools = (connection.tools as { name: string; description?: string; inputSchema: Record<string, unknown> }[]).map((entry) => {
        const modelName = createMcpToolName(name, entry.name, (n) => taken.has(n));
        taken.add(modelName);
        return { tool: entry.name, name: modelName, description: entry.description ?? "", inputSchema: entry.inputSchema };
      });
      return {
        tools,
        async call(tool, args) {
          const result = await connection.callTool(tool, args, { timeoutMs: connection.timeoutMs, signal: AbortSignal.timeout(connection.timeoutMs) });
          return { text: resultText(result), isError: result.isError === true };
        },
        close: () => connection.close(),
      };
    },
  };
}
