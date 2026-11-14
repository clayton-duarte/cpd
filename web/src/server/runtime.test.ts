import { createServer, type Server } from "node:http";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { once } from "node:events";
import { expect, it } from "vitest";
import { publishRuntime } from "./runtime.ts";

async function listening(server: Server): Promise<number> {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return (server.address() as import("node:net").AddressInfo).port;
}

it("keeps the old listener available until a ready standby is activated, then publishes the replacement", async () => {
  const home = mkdtempSync(join(tmpdir(), "cpd-handoff-"));
  const old = createServer((_req, res) => res.end("old"));
  const replacement = createServer((_req, res) => res.end("new"));
  try {
    const port = await listening(old);
    const stagedPort = await listening(replacement);
    await publishRuntime(replacement, home, "127.0.0.1", port, true);
    const record = () => JSON.parse(readFileSync(join(home, "run", "webui-prepared.json"), "utf8"));
    expect(record()).toMatchObject({ pid: process.pid, ready: true, phase: "staged" });
    expect(await (await fetch(`http://127.0.0.1:${port}`)).text()).toBe("old");
    expect(await (await fetch(`http://127.0.0.1:${stagedPort}`)).text()).toBe("new");
    const promoted = once(replacement, "listening");
    writeFileSync(join(home, "run", "webui-activate.pid"), String(process.pid));
    await new Promise<void>((resolve) => old.close(() => resolve()));
    await promoted;
    expect(record().phase).toBe("active");
    expect(readFileSync(join(home, "run", "webui.pid"), "utf8")).toBe(String(process.pid));
    expect(await (await fetch(`http://127.0.0.1:${port}`)).text()).toBe("new");
  } finally {
    await new Promise<void>((resolve) => replacement.close(() => resolve()));
    old.close();
    rmSync(home, { recursive: true, force: true });
  }
});
