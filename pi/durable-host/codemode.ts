// Contract §6.18 "Code mode": the namespaces of the attached MCP servers, `mcp__<server>__`. The host
// builds the code mode extension from these; no client imports @earendil-works/pi-codemode in a test.
// This is a stub: the test Job types the export, the implementation Job fills the body.

export type CodemodeServer = { server: string; tools: readonly string[] };

/** One namespace prefix per server, each to the tool names under it. Pure: it reads no registry, no config. */
export function namespacesFor(servers: readonly CodemodeServer[]): Record<string, readonly string[]> {
  throw new Error("not implemented");
}
