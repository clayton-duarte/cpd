import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PassThrough } from "node:stream";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PluginBoundaries } from "./plugin.ts";
import { createProdServer } from "./serve.ts";

const fake = vi.hoisted(() => ({
  request: undefined as undefined | ((req: IncomingMessage, res: ServerResponse) => void),
}));
vi.mock("node:http", () => ({
  createServer: (handler: typeof fake.request) => {
    fake.request = handler;
    return { once() {}, off() {}, listen(_port: number, _host: string, ready: () => void) { ready(); } };
  },
}));
vi.mock("./plugin.ts", () => ({
  createServices: () => ({ cockpit: { start: () => Promise.resolve() }, newConnection: vi.fn() }),
  realBoundaries: vi.fn(), cpdWebPlugin: vi.fn(),
}));
vi.mock("./ws.ts", () => ({ createWsServer: vi.fn(), createUpgradeFactory: vi.fn(), WS_PATH: "/cpd-ws" }));

let dir: string;
beforeEach(async () => {
  dir = mkdtempSync(join(tmpdir(), "cpd-static-paths-"));
  mkdirSync(join(dir, "public"));
  mkdirSync(join(dir, "public-private"));
  writeFileSync(join(dir, "public/index.html"), "public page");
  writeFileSync(join(dir, "public/asset file.js"), "public asset");
  writeFileSync(join(dir, "public-private/secret.txt"), "private sibling data");
  await createProdServer({ staticDir: join(dir, "public"), boundaries: {} as PluginBoundaries });
});
afterEach(() => rmSync(dir, { recursive: true, force: true }));

async function request(url: string, method = "GET"): Promise<{ status: number; body: string }> {
  let status = 0;
  const chunks: Buffer[] = [];
  const response = Object.assign(new PassThrough(), {
    writeHead(code: number) { status = code; return this; },
  });
  response.on("data", (chunk: Buffer) => chunks.push(chunk));
  const ended = new Promise<void>((resolve) => response.on("end", resolve));
  fake.request?.({ method, url, headers: { host: "localhost" } } as IncomingMessage,
    response as unknown as ServerResponse);
  await ended;
  return { status, body: Buffer.concat(chunks).toString() };
}

it("refuses encoded traversal into a sibling sharing the static directory prefix", async () => {
  const result = await request("/..%2fpublic-private/secret.txt");
  expect(result.status).toBe(403);
  expect(result.body).not.toContain("private sibling data");
});

it.each(["/%E0%A4%A", "/%00"])("returns 400 for malformed path %s and still serves later requests", async (url) => {
  expect((await request(url)).status).toBe(400);
  expect(await request("/")).toEqual({ status: 200, body: "public page" });
});

it("preserves encoded asset names, HEAD responses and SPA fallback", async () => {
  expect(await request("/asset%20file.js")).toEqual({ status: 200, body: "public asset" });
  expect(await request("/asset%20file.js", "HEAD")).toEqual({ status: 200, body: "" });
  expect(await request("/plans/example")).toEqual({ status: 200, body: "public page" });
  expect(await request("/plans/example", "HEAD")).toEqual({ status: 200, body: "" });
});
