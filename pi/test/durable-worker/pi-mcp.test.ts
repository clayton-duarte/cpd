import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { piMcpPort } from "../../durable-worker/pi-mcp.ts";

const FAKE_CONFIG = `export function loadMcpConfig() { return { servers: [], errors: [] }; }`;
const FAKE_TOOLS = `export function createMcpToolName(server, tool, isTaken) {
  const plain = ("mcp__" + server + "__" + tool).replace(/[^A-Za-z0-9_]/g, "_");
  return isTaken(plain) ? plain + "_h4sh" : plain;
}`;
const FAKE_RUNTIME = `
export function createDefaultTransport() {}
export class McpOAuthCredentialStore {}
export class McpServerConnection {
  constructor(options) {
    this.options = options;
    const f = globalThis.__fakeMcp;
    this.state = "connecting";
    this.tools = f.tools ?? [];
    this.timeoutMs = 4321;
    this.closed = 0;
    this.calls = [];
    f.constructed.push(this);
  }
  async getClient() {
    const f = globalThis.__fakeMcp;
    if (f.needsAuth) { this.state = "needs-auth"; if (f.rejectOnAuth) throw new Error("auth"); return undefined; }
    this.state = "connected";
    return {};
  }
  async callTool(name, args, options) {
    this.calls.push([name, args, options]);
    return globalThis.__fakeMcp.result;
  }
  async close() { this.closed++; }
}
export const markers = { createDefaultTransport, McpOAuthCredentialStore };`;

type Fake = { constructed: any[]; tools?: unknown[]; result?: unknown; needsAuth?: boolean; rejectOnAuth?: boolean };

function setup(t: { after(fn: () => void): void }, files: { runtime?: string | null } = {}) {
  const root = mkdtempSync(join(tmpdir(), "cpd-pi-mcp-"));
  t.after(() => {
    rmSync(root, { recursive: true, force: true });
    delete (globalThis as any).__fakeMcp;
  });
  const dist = join(root, "dist");
  mkdirSync(join(dist, "extensions/mcp"), { recursive: true });
  writeFileSync(join(dist, "index.js"), "");
  writeFileSync(join(dist, "extensions/mcp/config.js"), FAKE_CONFIG);
  writeFileSync(join(dist, "extensions/mcp/tools.js"), FAKE_TOOLS);
  if (files.runtime !== null) writeFileSync(join(dist, "extensions/mcp/runtime.js"), files.runtime ?? FAKE_RUNTIME);
  const fake: Fake = { constructed: [] };
  (globalThis as any).__fakeMcp = fake;
  return { fake, entry: join(dist, "index.js") };
}

test("the pi MCP port connects a server through pi's own connection class with the server entry and the cwd", { timeout: 10_000 }, async (t) => {
  const { fake, entry } = setup(t);
  const server = { url: "https://mcp.example/docs" };
  await piMcpPort(entry, "/work/dir").connect("docs", server);
  assert.equal(fake.constructed.length, 1);
  const o = fake.constructed[0].options;
  assert.deepEqual(o.entry, { name: "docs", config: server, source: "extension" });
  assert.equal(o.cwd, "/work/dir");
  assert.equal(typeof o.createTransport, "function");
  assert.equal(o.createTransport.name, "createDefaultTransport");
  assert.equal(o.credentials.constructor.name, "McpOAuthCredentialStore");
  assert.equal(typeof o.onTools, "function");
  assert.equal(fake.constructed[0].state, "connected");
});

test("the pi MCP port lists the tools with pi's model-facing name, the server's tool name, description and input schema", { timeout: 10_000 }, async (t) => {
  const { fake, entry } = setup(t);
  fake.tools = [
    { name: "get-issue", description: "d1", inputSchema: { type: "object" } },
    { name: "search", description: "d2", inputSchema: { type: "object", properties: { q: { type: "string" } } } },
  ];
  const c = await piMcpPort(entry, "/w").connect("docs", { url: "https://x" });
  assert.deepEqual(c.tools, [
    { tool: "get-issue", name: "mcp__docs__get_issue", description: "d1", inputSchema: { type: "object" } },
    { tool: "search", name: "mcp__docs__search", description: "d2", inputSchema: { type: "object", properties: { q: { type: "string" } } } },
  ]);

  const { fake: f2, entry: e2 } = setup(t);
  f2.tools = [
    { name: "a-b", description: "", inputSchema: { type: "object" } },
    { name: "a.b", description: "", inputSchema: { type: "object" } },
  ];
  const c2 = await piMcpPort(e2, "/w").connect("docs", { url: "https://x" });
  assert.deepEqual(c2.tools.map((x) => x.name), ["mcp__docs__a_b", "mcp__docs__a_b_h4sh"]);
});

test("calling a tool passes the server's tool name, the arguments and a signal, and returns the joined text blocks and isError", { timeout: 10_000 }, async (t) => {
  const { fake, entry } = setup(t);
  const c = await piMcpPort(entry, "/w").connect("docs", { url: "https://x" });
  fake.result = { content: [{ type: "text", text: "a" }, { type: "image" }, { type: "text", text: "b" }], isError: true };
  assert.deepEqual(await c.call("get-issue", { id: "A-1" }), { text: "a\nb", isError: true });
  const [name, args, options] = fake.constructed[0].calls[0];
  assert.equal(name, "get-issue");
  assert.deepEqual(args, { id: "A-1" });
  assert.ok(options.signal instanceof AbortSignal);
  assert.equal(options.timeoutMs, 4321);
  fake.result = { content: [{ type: "text", text: "ok" }] };
  assert.deepEqual(await c.call("get-issue", {}), { text: "ok", isError: false });
});

test("a connection that needs sign-in is refused with a message naming pi mcp login", { timeout: 10_000 }, async (t) => {
  for (const rejectOnAuth of [false, true]) {
    const { fake, entry } = setup(t);
    fake.needsAuth = true;
    fake.rejectOnAuth = rejectOnAuth;
    await assert.rejects(piMcpPort(entry, "/w").connect("docs", { url: "https://x" }), (e: Error) => e.message.includes("pi mcp login docs"));
    assert.equal(fake.constructed[0].closed, 1);
  }
});

test("a pi module that is missing or lacks an export is refused with the module named", { timeout: 10_000 }, async (t) => {
  const missing = setup(t, { runtime: null });
  await assert.rejects(piMcpPort(missing.entry, "/w").connect("docs", {}), (e: Error) => e.message.includes("runtime.js"));
  const lacking = setup(t, { runtime: "export function createDefaultTransport() {}\nexport class McpOAuthCredentialStore {}\n" });
  await assert.rejects(piMcpPort(lacking.entry, "/w").connect("docs", {}), (e: Error) => /McpServerConnection|runtime\.js/.test(e.message));
});

test("closing the connection closes pi's connection", { timeout: 10_000 }, async (t) => {
  const { fake, entry } = setup(t);
  const c = await piMcpPort(entry, "/w").connect("docs", { url: "https://x" });
  await c.close();
  assert.equal(fake.constructed[0].closed, 1);
});
