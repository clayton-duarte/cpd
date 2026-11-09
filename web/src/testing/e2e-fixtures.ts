import { createServer } from "node:http";
import { chmodSync, createReadStream, existsSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { test as base, expect } from "@playwright/test";
import type { ElectronApplication, Page } from "playwright";
import { _electron as electron } from "playwright";

/** The web/ package root (two levels up from web/src/testing/), the default `CPD_CHECKOUT`. */
export const webRoot = fileURLToPath(new URL("../..", import.meta.url));

/** The pinned Electron binary the app runs on (the `electron` devDependency). */
const electronExecutable = createRequire(import.meta.url)("electron") as string;

/** The scripted fake cpd (`e2e-fake-cpd`) the data plane boots against, never a real cpd. */
const fakeCpdSource = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";

/**
 * Stage the suite's private data plane in `staging`: a `bin` dir holding a `cpd` symlink to the
 * scripted fake cpd, and a fake login shell that reports the app's PATH unchanged (the app runs
 * `$SHELL -ilc` to resolve the login path, and the real shell would drop the fake dir, so the pty's
 * `cpd tui --attach` would spawn a real cpd). Sets CPD_BIN, SHELL and PATH so the app spawns the
 * fake only. The caller removes `staging` in `afterAll`.
 */
export function stageFakeCpd(staging: string): void {
  const fakeBin = join(staging, "bin");
  const fakeShell = join(staging, "shell");
  mkdirSync(fakeBin, { recursive: true });
  writeFileSync(
    fakeShell,
    `#!/bin/sh\nprintf '%s' '__CPD_PATH_BEGIN__'; printf '%s' "$PATH"; printf '%s' '__CPD_PATH_END__'\n`,
  );
  chmodSync(fakeShell, 0o755);
  symlinkSync(fakeCpdSource, join(fakeBin, "cpd"));
  process.env["CPD_BIN"] = fakeCpdSource;
  process.env["SHELL"] = fakeShell;
  process.env["PATH"] = `${fakeBin}:${basePath}`;
}

/**
 * Headless Electron on Linux needs these: the bundled `chrome-sandbox` is not SUID (--no-sandbox),
 * a virtual display has no GPU (--disable-gpu), and the CI runner's /dev/shm may be small
 * (--disable-dev-shm-usage). macOS keeps its plain launch: the window is hidden and isolated, and
 * these switches are neither needed nor harmless there.
 */
export const headlessLinuxArgs: string[] =
  process.platform === "linux" ? ["--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage"] : [];

type LaunchAppOptions = {
  /** A per-run `CPD_HOME`, isolated and removed by the caller after the run. */
  cpdHome: string;
  /** A per-run Electron `userData` dir, isolated and removed by the caller after the run. */
  userDataDir: string;
  /** The renderer URL the app is told to load (`CPD_ELECTRON_DEV_URL`). */
  rendererUrl: string;
  /**
   * The Electron app path (the one non-flag argument). Defaults to `"."`, the checkout's `web/`,
   * where every existing scenario runs. A scenario that stages an installed app dir passes its
   * absolute path so the app is launched from that dir (`app.getAppPath()` is the staged dir).
   */
  appPath?: string;
  /** Extra env for a scenario (for example `CPD_CHECKOUT`); it never overrides the hidden/isolation keys. */
  env?: Record<string, string>;
  /**
   * Create the window hidden (`CPD_ELECTRON_HIDDEN=1`). Default true: the suite never shows a
   * window on the User's screen (air-d68). The shared fixture passes false on Linux (the CI runner
   * has no User screen under xvfb), while the view spec keeps the default so its "no window is
   * visible" assertion still holds.
   */
  hidden?: boolean;
};

/**
 * The house pattern for a scenario's fake `cpd` on PATH: a staged `bin` dir holding a `cpd`
 * symlink to `cpdPath`, first on PATH, behind a fake login shell that reports PATH unchanged (so
 * `resolveLoginPath`'s `$SHELL -ilc` keeps the staged dir first). Sets `CPD_BIN`, `SHELL` and
 * `PATH` on `process.env`, which the launched app inherits. Exported so a scenario that boots the
 * app against its own staged checkout reuses the exact setup instead of copying it.
 */
export function stageFakeBin(staging: string, cpdPath: string): { bin: string; shell: string } {
  const bin = join(staging, "bin");
  const shell = join(staging, "shell");
  mkdirSync(bin, { recursive: true });
  writeFileSync(
    shell,
    `#!/bin/sh\nprintf '%s' '__CPD_PATH_BEGIN__'; printf '%s' "$PATH"; printf '%s' '__CPD_PATH_END__'\n`,
  );
  chmodSync(shell, 0o755);
  symlinkSync(cpdPath, join(bin, "cpd"));
  process.env["CPD_BIN"] = cpdPath;
  process.env["SHELL"] = shell;
  process.env["PATH"] = `${bin}:${process.env["PATH"] ?? ""}`;
  return { bin, shell };
}

/**
 * The one launcher for the suite's Electron app. It pins what every launch must have: the Linux
 * headless flags (`headlessLinuxArgs`), isolation from the User's real profile (a temp
 * `--user-data-dir` and `CPD_HOME`), a hidden window by default (`CPD_ELECTRON_HIDDEN`; air-d68: the
 * suite never shows a window on the User's screen), and the `[boot] <step>` trace so a boot that
 * never reaches loadDevUrl names its blocking step in the output. A scenario adds its own seams
 * through `env`/`hidden`, and the app's own stdout/stderr is surfaced in the Playwright output so a
 * boot crash is visible instead of a bare firstWindow timeout.
 */
export async function launchApp(options: LaunchAppOptions): Promise<ElectronApplication> {
  const hidden = options.hidden ?? true;
  const app = await electron.launch({
    executablePath: electronExecutable,
    // `--user-data-dir` isolates Electron's own state (the single-instance lock, caches and app
    // data) in a temp dir, so the run never touches the User's real profile. The last argument is
    // the Electron app path: `"."` is the checkout's `web/`, or the staged installed app dir.
    args: [...headlessLinuxArgs, `--user-data-dir=${options.userDataDir}`, options.appPath ?? "."],
    cwd: webRoot,
    env: {
      ...process.env,
      ...options.env,
      CPD_HOME: options.cpdHome,
      // Create the app window hidden: no window ever appears on the User's screen.
      ...(hidden ? { CPD_ELECTRON_HIDDEN: "1" } : {}),
      CPD_MODE: "preview",
      CPD_ELECTRON_DEV_URL: options.rendererUrl,
      // Print each boot step, so a boot that stops before loadDevUrl names it in the CI log.
      CPD_BOOT_TRACE: "1",
    },
  });
  const child = app.process();
  child.stdout?.on("data", (chunk: Buffer) => process.stdout.write(`[electron] ${chunk.toString()}`));
  child.stderr?.on("data", (chunk: Buffer) => process.stderr.write(`[electron] ${chunk.toString()}`));
  return app;
}

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
    const app = await launchApp({
      cpdHome,
      userDataDir,
      rendererUrl: rendererServer.url,
      env: { CPD_CHECKOUT: webRoot },
      // air-d68: hidden on macOS (the User's screen). On Linux the CI runner has no User screen
      // under xvfb, so the window may be shown there.
      hidden: process.platform !== "linux",
    });
    await use(app);
    await app.close();
  },

  page: async ({ electronApp }, use) => {
    const page = await firstWindow(electronApp);
    await page.waitForLoadState("domcontentloaded");
    await use(page);
  },
});

/**
 * The app's first window. `firstWindow()` waits for the "window" event; if that event does not fire
 * (or fired before the fixture ran) fall back to polling `windows()` — no sleep.
 */
export async function firstWindow(app: ElectronApplication): Promise<Page> {
  try {
    return await app.firstWindow({ timeout: 20_000 });
  } catch (error) {
    console.log(`firstWindow did not fire (${(error as Error).message}); polling electronApp.windows()`);
    await expect.poll(() => app.windows().length, { timeout: 30_000 }).toBeGreaterThan(0);
    return app.windows()[0]!;
  }
}

export { expect };
