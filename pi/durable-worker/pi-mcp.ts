import { createMcpToolName } from "@earendil-works/pi-mcp";
import { loadPi, resultText } from "../lib/slack-channel.ts";
import type { McpPort } from "./mcp.ts";

/** The durable runner's McpPort over pi's own MCP client. */
export function piMcpPort(codingAgentEntry: string, cwd: string): McpPort {
  return {
    async connect(name, server) {
      const pi = await loadPi(codingAgentEntry);
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
