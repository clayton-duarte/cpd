import * as http from "node:http";
import * as fs from "node:fs";
import * as path from "node:path";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeScheduler } from "../testing/scheduler.ts";
import type { PluginBoundaries } from "./plugin.ts";
import { createProdServer, findStaticDir } from "./serve.ts";

const fakeBoundaries = (cpdHome: string): PluginBoundaries =>
  ({
    cpdExec: {
      run: () => Promise.resolve({ stdout: "[]", stderr: "", exitCode: 0 }),
    },
    follow: {
      start: () => {
        throw new Error("follow not used in test");
      },
    },
    socketFactory: () => {
      throw new Error("no sockets in tests");
    },
    fs: {},
    watch: {},
    git: { branch: () => Promise.resolve(null) },
    scheduler: new FakeScheduler(),
    cpdHome,
    webuiToken: "fake-token",
  }) as unknown as PluginBoundaries;

describe("serve-browser", () => {
  let tempDir: string;
  let server: http.Server | null = null;

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(tmpdir(), "serve-browser-test-"));
  });

  afterEach(async () => {
    if (server) {
      await new Promise<void>((resolve) => server!.close(() => resolve()));
      server = null;
    }
    vi.unstubAllEnvs();
    fs.rmSync(tempDir, { recursive: true, force: true });
  });

  it("protects the production HTTP entry with its backend credential", async () => {
    const token = "backend-token-".repeat(4);
    vi.stubEnv("CPD_WEBUI_ACCESS_TOKEN", token);
    fs.writeFileSync(path.join(tempDir, "index.html"), "private shell");
    server = await createProdServer({ port: 0, host: "127.0.0.1", staticDir: tempDir, boundaries: fakeBoundaries(tempDir) });
    const port = (server.address() as import("node:net").AddressInfo).port;
    const url = `http://127.0.0.1:${port}/`;
    const rejected = await fetch(url);
    expect(rejected.status).toBe(401);
    expect(rejected.headers.get("www-authenticate")).toContain("Basic");
    expect(await rejected.text()).not.toContain("private shell");
    const accepted = await fetch(url, { headers: { Authorization: `Basic ${Buffer.from(`cpd:${token}`).toString("base64")}` } });
    expect(accepted.status).toBe(200);
    expect(await accepted.text()).toBe("private shell");
  });

  it("prod server serves index.html without electron bridge", async () => {
    // 1. Static dir resolution in production must point to the browser build root (web/dist),
    // not dist-electron/renderer (CONTRACT.md §15.1, webui-network-ports.md, pro-d552).
    const resolvedDir = findStaticDir();
    expect(resolvedDir).toMatch(/dist$/);
    expect(resolvedDir).not.toMatch(/dist-electron/);

    // 2. Setup a static directory representing browser build artifacts
    const staticDir = path.join(tempDir, "dist");
    fs.mkdirSync(staticDir, { recursive: true });

    // Browser build output has index.html referencing main.tsx (not electron.html or renderer.ts)
    const indexHtml = [
      "<!doctype html>",
      "<html>",
      "  <head><title>CPD</title></head>",
      "  <body>",
      '    <div id="root"></div>',
      '    <script type="module" src="/src/main.tsx"></script>',
      "  </body>",
      "</html>",
    ].join("\n");
    fs.writeFileSync(path.join(staticDir, "index.html"), indexHtml);

    // Create an arbitrary port so we don't bind 3333 in tests
    const boundaries = fakeBoundaries(tempDir);
    server = await createProdServer({
      port: 0,
      host: "127.0.0.1",
      staticDir,
      boundaries,
    });

    const addr = server.address();
    expect(addr).not.toBeNull();
    const port = typeof addr === "object" && addr ? addr.port : 0;

    // Fetch GET /
    const rootRes = await fetch(`http://127.0.0.1:${port}/`);
    expect(rootRes.status).toBe(200);
    const rootBody = await rootRes.text();
    expect(rootBody).toBe(indexHtml);
    expect(rootBody).toContain("/src/main.tsx");
    expect(rootBody).not.toContain("window.cpdElectron");
    expect(rootBody).not.toContain("renderer.ts");
    expect(rootBody).not.toContain("file:///");

    // Fetch client-side routed path (e.g. /plan/p1) should fall back to index.html (not electron.html)
    const routeRes = await fetch(`http://127.0.0.1:${port}/plan/p1`);
    expect(routeRes.status).toBe(200);
    const routeBody = await routeRes.text();
    expect(routeBody).toBe(indexHtml);
    expect(routeBody).toContain("/src/main.tsx");
  });
});
