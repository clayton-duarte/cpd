import { createServer } from "node:http";
import { createReadStream, existsSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { test as base, expect } from "@playwright/test";
import type { ElectronApplication, Page } from "playwright";
import { _electron as electron } from "playwright";

/** The web/ package root (two levels up from web/src/testing/). */
const webRoot = fileURLToPath(new URL("../..", import.meta.url));

/** The pinned Electron binary the app runs on (the `electron` devDependency). */
const electronExecutable = createRequire(import.meta.url)("electron") as string;

/** The built renderer the suite points the app at; overridable for a scenario that serves its own. */
const rendererDir = process.env["CPD_E2E_RENDERER_DIR"] ?? join(webRoot, "dist-electron", "renderer");

const CONTENT_TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
};

export type RendererServer = {
  /** The renderer URL the app is told to load (`CPD_ELECTRON_DEV_URL`). */
  readonly url: string;
  readonly close: () => Promise<void>;
};

/**
 * Owns the renderer URL: a static server on an ephemeral port serving the built
 * renderer from `root`, so `CPD_MODE=preview` + `CPD_ELECTRON_DEV_URL` load a
 * page the suite controls instead of a shared dev server.
 */
export async function startRendererServer(root: string): Promise<RendererServer> {
  const server = createServer((request, response) => {
    const requestPath = new URL(request.url ?? "/", "http://127.0.0.1").pathname;
    const filePath = resolve(root, `.${requestPath === "/" ? "/electron.html" : requestPath}`);
    const inRoot = filePath === root || filePath.startsWith(root + sep);
    if (!inRoot || !existsSync(filePath)) {
      response.writeHead(404).end("not found");
      return;
    }
    response.writeHead(200, { "content-type": CONTENT_TYPES[extname(filePath)] ?? "application/octet-stream" });
    createReadStream(filePath).pipe(response);
  });
  await new Promise<void>((listening, failed) => {
    server.once("error", failed);
    server.listen(0, "127.0.0.1", listening);
  });
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("renderer server has no port");
  return {
    url: `http://127.0.0.1:${address.port}/electron.html`,
    close: () => new Promise<void>((closed) => server.close(() => closed())),
  };
}

type Fixtures = {
  /** A per-run `CPD_HOME`, isolated and removed after the test. */
  cpdHome: string;
  /** A per-run Electron `userData` dir, isolated and removed after the test. */
  userDataDir: string;
  /** The renderer URL server, owned by the suite. */
  rendererServer: RendererServer;
  /** The launched desktop app. */
  electronApp: ElectronApplication;
  /** The app's first window. */
  page: Page;
};

export const test = base.extend<Fixtures>({
  cpdHome: async ({}, use) => {
    const home = await mkdtemp(join(tmpdir(), "cpd-e2e-home-"));
    await use(home);
    await rm(home, { recursive: true, force: true });
  },

  userDataDir: async ({}, use) => {
    const dir = await mkdtemp(join(tmpdir(), "cpd-e2e-userdata-"));
    await use(dir);
    await rm(dir, { recursive: true, force: true });
  },

  rendererServer: async ({}, use) => {
    const server = await startRendererServer(rendererDir);
    await use(server);
    await server.close();
  },

  electronApp: async ({ cpdHome, userDataDir, rendererServer }, use) => {
    const app = await electron.launch({
      executablePath: electronExecutable,
      // `--user-data-dir` isolates Electron's own state (the single-instance lock, caches and
      // app data) in a temp dir, so the run never touches the User's real profile.
      args: [`--user-data-dir=${userDataDir}`, "."],
      cwd: webRoot,
      env: {
        ...process.env,
        CPD_HOME: cpdHome,
        // Create the app window hidden: no window ever appears on the User's screen.
        CPD_ELECTRON_HIDDEN: "1",
        CPD_MODE: "preview",
        CPD_CHECKOUT: webRoot,
        CPD_ELECTRON_DEV_URL: rendererServer.url,
      },
    });
    await use(app);
    await app.close();
  },

  page: async ({ electronApp }, use) => {
    const page = await electronApp.firstWindow();
    await page.waitForLoadState("domcontentloaded");
    await use(page);
  },
});

export { expect };
