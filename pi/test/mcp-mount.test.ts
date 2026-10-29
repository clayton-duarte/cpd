import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { mcpMount } from "../lib/mcp.ts";
import { transportsStarted } from "./fake.ts";
import { harness, writeMcpJson } from "./mcp-harness.ts";

// contract §6.11 (pro-d3, pro-d7): the adapter mounts pi's own MCP extension with an empty file config, so no `mcp.json` reaches a Session and a server registered with
// `pi.registerMcpServer` is connected by pi's code. Real pi 1.3.0 (the pinned dependency), a recording transport that never connects: no network, no model call.
// Each test also registers `slack` with an inline factory: that is the control that the harness connects what is registered, and it is what is red against the stub.

const SLACK = { url: "https://mcp.slack.com/mcp" };
const EVIL = { url: "https://evil.example/mcp" };
const registerSlack = (pi: { registerMcpServer(name: string, config: typeof SLACK): void }) => pi.registerMcpServer("slack", SLACK);

test("the mount connects no server from the user-level mcp.json", async () => {
  const h = harness();
  await h.run(async () => {
    writeMcpJson(h.agentDir, { evil: EVIL });
    await h.session([mcpMount({ createTransport: h.createTransport }), (pi) => registerSlack(pi)]);
    await transportsStarted(h.transports); // pi connects every server it knows at `session_start`, `evil` with `slack`
    assert.deepEqual(h.transports.map((t) => t.name), ["slack"], "only the registered server gets a transport, never `evil` from <agentDir>/mcp.json");
  });
});

test("the mount connects no server from a trusted project's .pi/mcp.json", async () => {
  const h = harness();
  await h.run(async () => {
    writeMcpJson(join(h.cwd, ".pi"), { evil: EVIL });
    // The project is made trusted through `resourceLoader.reload({ resolveProjectTrust })` (mcp-harness.ts `trusted`): that is what pi's CLI does after its prompt.
    await h.session([mcpMount({ createTransport: h.createTransport }), (pi) => registerSlack(pi)], { trusted: true });
    await transportsStarted(h.transports); // pi connects every server it knows at `session_start`, `evil` with `slack`
    assert.deepEqual(h.transports.map((t) => t.name), ["slack"], "only the registered server gets a transport, never `evil` from <cwd>/.pi/mcp.json");
  });
});

test("the mount connects a server registered with registerMcpServer", async () => {
  const h = harness();
  await h.run(async () => {
    await h.session([mcpMount({ createTransport: h.createTransport }), (pi) => registerSlack(pi)]);
    await transportsStarted(h.transports);
    assert.equal(h.transports.length, 1);
    assert.equal(h.transports[0]!.name, "slack");
    assert.deepEqual(h.transports[0]!.config, SLACK);
  });
});

test("pi/package.json lists mcp-extension.ts as an extension and it default-exports the mount", async () => {
  const dir = join(import.meta.dirname, "..");
  const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8")) as { pi: { extensions: string[] } };
  assert.ok(pkg.pi.extensions.includes("./mcp-extension.ts"), `pi.extensions lacks ./mcp-extension.ts: ${JSON.stringify(pkg.pi.extensions)}`);
  const mod = (await import("../mcp-extension.ts")) as { default: unknown };
  assert.equal(typeof mod.default, "function", "the default export is the mount's factory");
});
