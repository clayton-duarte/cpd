// Contract §6.18 "Code mode": the namespaces of the attached MCP servers, `mcp__<server>__`. The host
// builds the code mode extension from these; no client imports @earendil-works/pi-codemode in a test.

export type CodemodeServer = { server: string; tools: readonly string[] };

/** One namespace prefix per server, each to the tool names under it. Pure: it reads no registry, no config. */
export function namespacesFor(servers: readonly CodemodeServer[]): Record<string, readonly string[]> {
  const out: Record<string, readonly string[]> = {};
  for (const { server, tools } of servers) out[`mcp__${server}__`] = tools;
  return out;
}

/** `namespacesFor` as `createCodemodeExtension` reads them: one `{ name, prefix }` per server (contract §6.18). */
export function codemodeNamespaces(servers: readonly CodemodeServer[]): { name: string; prefix: string }[] {
  return Object.keys(namespacesFor(servers)).map((prefix) => ({ name: prefix.slice("mcp__".length, -"__".length), prefix }));
}
