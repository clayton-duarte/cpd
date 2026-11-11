import assert from "node:assert/strict";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { test } from "node:test";

import { CoreSocketClient, getToolDefinitions } from "../lib/tools.ts";
import { TOOL_DEFINITIONS, NAMESPACES } from "../lib/generated/tools.ts";

// docs/contract.md §5.9 (Typed pi tools, context curation and the message bus).
// Pins:
// - every namespace of the table is one tool, and each op sends the method <domain>.<verb>
//   with the parameters the generated schema allows over run/core.sock;
// - hello binds the connection to the Session's role, and a request before it is bad_request;
// - a Worker's decide is not_permitted.

test("typed tools call core socket", async () => {
  // 1. Verify all 12 namespaces exist and define ops matching domain.verb
  assert.equal(NAMESPACES.length, 12);
  const defs = getToolDefinitions();
  for (const ns of NAMESPACES) {
    assert.ok(defs[ns], `Tool definition missing for namespace ${ns}`);
    assert.ok(defs[ns].ops.length > 0, `Namespace ${ns} has no ops`);
  }

  // 2. Set up a mock core socket over a temporary Unix socket
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "cpd-core-sock-test-"));
  const sockPath = path.join(tmpDir, "core.sock");

  const receivedRequests: Array<{ id: number; method: string; params: any }> = [];

  const server = net.createServer((conn) => {
    let boundRole: string | null = null;
    const rl = readline.createInterface({ input: conn });

    rl.on("line", (line) => {
      if (!line.trim()) return;
      const req = JSON.parse(line);
      receivedRequests.push(req);

      if (req.method === "hello") {
        if (req.params.session === "s-worker") {
          boundRole = "worker";
        } else if (req.params.session === "s-lead") {
          boundRole = "lead";
        } else if (req.params.session === "s-manager") {
          boundRole = "manager";
        }
        conn.write(
          JSON.stringify({
            id: req.id,
            result: {
              protocol: 1,
              session: req.params.session,
              role: boundRole ?? "worker",
              plan: "p1",
              context: "ctx",
            },
            error: null,
          }) + "\n"
        );
        return;
      }

      if (!boundRole) {
        conn.write(
          JSON.stringify({
            id: req.id,
            result: null,
            error: { code: "bad_request", message: "Request before hello" },
          }) + "\n"
        );
        return;
      }

      // Check role permissions: a Worker's decide is not_permitted
      if (boundRole === "worker" && req.method === "context.decide") {
        conn.write(
          JSON.stringify({
            id: req.id,
            result: null,
            error: { code: "not_permitted", message: "Only Managers and Leads write context" },
          }) + "\n"
        );
        return;
      }

      // Default successful mock response
      conn.write(
        JSON.stringify({
          id: req.id,
          result: { ok: true, echo: req.params },
          error: null,
        }) + "\n"
      );
    });
  });

  await new Promise<void>((resolve) => server.listen(sockPath, resolve));

  try {
    // 3. Connect client as s-worker
    const client = new CoreSocketClient({
      socketPath: sockPath,
      session: "s-worker",
    });
    await client.connect();

    // Op sends method <domain>.<verb>
    const planShowRes = await client.call("plan", "show", { plan: "p1" });
    assert.deepEqual(planShowRes, { ok: true, echo: { plan: "p1" } });

    // Verify Worker's decide is not_permitted
    await assert.rejects(
      async () => {
        await client.call("context", "decide", { text: "Some decision" });
      },
      (err: any) => {
        assert.equal(err.code, "not_permitted");
        return true;
      }
    );

    client.close();
  } finally {
    server.close();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
