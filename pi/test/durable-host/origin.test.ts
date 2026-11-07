import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { CONFIG, withHost } from "./rig.ts";

/**
 * commands.md §15.3 "The user-message tag" (Plan ui-origin): the durable host's user content. A cpd-web client's
 * socket submit is the WebUI surface, so its content is tagged `[via WebUI · view: <view>] ` (the durable-webui-turn
 * artefact: the socket submit → `deps.submit` → `conversation.submit` content builder in run.ts). The view is read
 * from the marker in the host's own `$CPD_HOME`.
 */

test("a cpd-web socket submit is tagged via WebUI", { timeout: 20_000 }, async (t) => {
  const home = mkdtempSync(join(tmpdir(), "cpd-origin-"));
  const saved = process.env.CPD_HOME;
  process.env.CPD_HOME = home; // the host reads its own $CPD_HOME; never the real User's marker
  t.after(() => {
    if (saved === undefined) delete process.env.CPD_HOME;
    else process.env.CPD_HOME = saved;
    rmSync(home, { recursive: true, force: true });
  });
  mkdirSync(join(home, "run"), { recursive: true });
  writeFileSync(join(home, "run", "ui-view.json"), JSON.stringify({ view: "webui", pid: process.pid }));

  await withHost({ config: { ...CONFIG, home } }, async (r) => {
    await r.up();
    const client = r.connect();
    client.send({ type: "hello", protocol: 2, min_protocol: 2, client: "cpd-web" });
    client.send({ type: "attach", req: "attach-1" });
    await r.drain();
    client.send({ type: "submit", req: "s1", text: "hi", when_busy: "followUp" });
    await r.until(() => r.submitted.length >= 1);
    const input = r.submitted[0] as { content: unknown };
    assert.equal(input.content, "[via WebUI · view: WebUI] hi");
  });
});
