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
  stream: undefined as PassThrough | undefined,
}));
vi.mock("node:fs", async (original) => {
  const fs = await original<typeof import("node:fs")>();
  return { ...fs, createReadStream: (...args: Parameters<typeof fs.createReadStream>) => fake.stream ?? fs.createReadStream(...args) };
});
vi.mock("node:http", () => ({
  createServer: (handler: typeof fake.request) => {
    fake.request = handler;
    return { once() {}, listen(_port: number, _host: string, ready: () => void) { ready(); } };
  },
}));
vi.mock("./plugin.ts", () => ({
  createServices: () => ({ cockpit: { start: () => Promise.resolve() }, newConnection: vi.fn() }),
  realBoundaries: vi.fn(), cpdWebPlugin: vi.fn(),
}));
vi.mock("./ws.ts", () => ({ createWsServer: vi.fn(), createUpgradeFactory: vi.fn(), WS_PATH: "/cpd-ws" }));

let dir: string;
beforeEach(async () => {
  fake.stream = undefined;
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

it.each(["/asset%20file.js", "/plans/example"])("contains an initial read failure for %s and serves the next request", async (url) => {
  const stream = new PassThrough();
  fake.stream = stream;
  const result = request(url);
  await vi.waitFor(() => expect(stream.listenerCount("error")).toBeGreaterThan(0));
  stream.emit("error", new Error("EACCES"));
  expect(await result).toEqual({ status: 500, body: "Internal Server Error" });
  fake.stream = undefined;
  expect(await request("/")).toEqual({ status: 200, body: "public page" });
});

it("terminates a partial response and releases the reader on client close", async () => {
  const stream = new PassThrough();
  fake.stream = stream;
  const response = Object.assign(new PassThrough(), {
    headersSent: false,
    writeHead() { this.headersSent = true; return this; },
  });
  fake.request?.({ method: "GET", url: "/asset%20file.js", headers: { host: "localhost" } } as IncomingMessage,
    response as unknown as ServerResponse);
  await vi.waitFor(() => expect(stream.listenerCount("error")).toBeGreaterThan(0));
  stream.emit("open");
  stream.write("partial");
  stream.emit("error", new Error("read failed"));
  expect(response.destroyed).toBe(true);
  await vi.waitFor(() => expect(stream.destroyed).toBe(true));
});

it("destroys a pending reader when the client disconnects before open", async () => {
  const stream = new PassThrough();
  fake.stream = stream;
  const response = Object.assign(new PassThrough(), { writeHead: vi.fn() });
  fake.request?.({ method: "GET", url: "/asset%20file.js", headers: { host: "localhost" } } as IncomingMessage,
    response as unknown as ServerResponse);
  await vi.waitFor(() => expect(stream.listenerCount("error")).toBeGreaterThan(0));
  response.destroy();
  await vi.waitFor(() => expect(stream.destroyed).toBe(true));
  stream.emit("open");
  expect(response.writeHead).not.toHaveBeenCalled();
});
