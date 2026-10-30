export type McpConnection = {
  tools: readonly { tool: string; name: string; description: string; inputSchema: Record<string, unknown> }[];
  call(tool: string, args: Record<string, unknown>): Promise<{ text: string; isError: boolean }>;
  close(): Promise<void>;
};

export type McpPort = { connect(name: string, server: Record<string, unknown>): Promise<McpConnection> };
