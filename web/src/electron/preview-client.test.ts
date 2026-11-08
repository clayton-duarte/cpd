import { expect, it } from "vitest";
import type { HealthRecord } from "./health.ts";
import {
  createPreviewClient,
  PREVIEW_READINESS_TIMEOUT_MS,
  type PreviewClientDeps,
} from "./preview-client.ts";

const T0 = Date.parse("2026-01-01T00:00:00.000Z");
const iso = (ms: number) => new Date(ms).toISOString();
const ROOT = "/repo";
const WORKTREE = "/repo.preview.web";
const DEV_URL = "http://localhost:5173/electron.html";
const MARKER = "/home/.cpd/run/ui-view.json";
const PID = 4242;

function loading(epoch = 1): HealthRecord {
  return { status: "loading", reloadEpoch: epoch, pid: PID, startedAt: iso(T0) };
}

function healthy(epoch = 1): HealthRecord {
  return { status: "healthy", reloadEpoch: epoch, pid: PID, startedAt: iso(T0), healthyAt: iso(T0) };
}

interface RunCall {
  readonly file: string;
  readonly args: readonly string[];
  readonly cwd: string;
  readonly env: Readonly<Record<string, string | undefined>>;
}

function setup(
  overrides: {
    code?: number;
    stderr?: string;
    runError?: Error;
    previous?: "tui" | "webui" | "webui-preview";
    serverReady?: boolean;
  } = {},
) {
  const calls: RunCall[] = [];
  const healthWrites: HealthRecord[] = [];
  const loadingShown: string[] = [];
  const loaded: string[] = [];
  const dialogs: string[] = [];
  const modes: string[] = [];
  const removed: string[] = [];
  const state = { clock: T0, health: loading() as HealthRecord | null };
  const deps: PreviewClientDeps = {
    root: ROOT,
    worktree: WORKTREE,
    devUrl: DEV_URL,
    pid: PID,
    now: () => state.clock,
    env: { CPD_MODE: "preview", CPD_CHECKOUT: WORKTREE },
    markerFile: MARKER,
    run: async (file, args, options) => {
      calls.push({ file, args, cwd: options.cwd, env: options.env });
      if (overrides.runError) throw overrides.runError;
      return { code: overrides.code ?? 0, stderr: overrides.stderr ?? "" };
    },
    readHealth: async () => state.health,
    writeHealth: async (record) => {
      healthWrites.push(record);
    },
    isPreviewServerReady: async () => overrides.serverReady ?? true,
    showLoading: () => {
      loadingShown.push("loading");
    },
    loadDevUrl: async (url) => {
      loaded.push(url);
    },
    // The failure port. main.ts wires it to the cockpit toast (CONTRACT.md §19.3), never a native
    // modal; the client only has to report a failure here.
    showErrorBox: (title, content) => {
      dialogs.push(`${title}: ${content}`);
    },
    setMode: async (mode) => {
      modes.push(mode);
    },
    previousMode: () => overrides.previous ?? "webui",
    removeMarker: async () => {
      removed.push(MARKER);
    },
    watchdog: { tick: async () => {} },
  };
  return { deps, calls, healthWrites, loadingShown, loaded, dialogs, modes, removed, state };
}

it("starts the serve-only preview server once and shows the loading page", async () => {
  const s = setup();
  const client = createPreviewClient(s.deps);
  await client.open();
  await client.open();
  expect(s.calls).toHaveLength(1);
  expect(s.calls[0]).toMatchObject({ file: "sh", args: [`${ROOT}/scripts/preview-web.sh`, "serve", WORKTREE] });
  expect(s.calls[0]!.args.some((arg) => arg.includes("electron") || arg.includes("watchdog"))).toBe(false);
  expect(s.healthWrites).toEqual([{ status: "loading", reloadEpoch: 1, pid: PID, startedAt: iso(T0) }]);
  expect(s.loadingShown).toEqual(["loading"]);
  expect(s.dialogs).toEqual([]);
});

it("reaches the dev URL without the running app being healthy", async () => {
  const s = setup();
  const client = createPreviewClient(s.deps);
  await client.open();
  // The running app's health never arrives: the old gate ("status === healthy") waited for the
  // renderer the dev-URL load itself starts — the circular wait. Readiness is the preview server.
  s.state.health = null;
  await client.tick();
  expect(s.loaded).toEqual([DEV_URL]);
  await client.tick();
  expect(s.loaded).toEqual([DEV_URL]);
});

it("a preview that never becomes ready times out and reports a failure", async () => {
  const s = setup({ serverReady: false, previous: "tui" });
  const client = createPreviewClient(s.deps);
  await client.open();
  // Advance well past the bounded readiness wait and keep ticking: the wait must end in a failure,
  // not hang.
  for (let i = 0; i < 3; i += 1) {
    s.state.clock += PREVIEW_READINESS_TIMEOUT_MS + 1;
    await client.tick();
  }
  expect(s.loaded).toEqual([]);
  expect(s.dialogs).toHaveLength(1);
  expect(s.modes).toEqual(["tui"]);
});

it("shows a native dialog and returns to the previous view when the server fails", async () => {
  const s = setup({ code: 1, stderr: "vite is down", previous: "tui" });
  const client = createPreviewClient(s.deps);
  await client.open();
  expect(s.dialogs).toEqual(["WebUI Preview: vite is down"]);
  expect(s.modes).toEqual(["tui"]);
  expect(s.loadingShown).toEqual([]);
  expect(s.healthWrites).toEqual([]);
});

it("keeps the server when switching away and stops it on quit", async () => {
  const s = setup();
  const client = createPreviewClient(s.deps);
  await client.open();
  await client.leave();
  expect(s.calls.filter((call) => call.args.includes("serve"))).toHaveLength(1);
  expect(s.calls.filter((call) => call.args.includes("unserve"))).toHaveLength(0);
  expect(s.removed).toEqual([]);
  await client.close();
  expect(s.calls.filter((call) => call.args.includes("unserve"))).toHaveLength(1);
  expect(s.calls.at(-1)!.args).toEqual([`${ROOT}/scripts/preview-web.sh`, "unserve"]);
  expect(s.removed).toEqual([MARKER]);
  await client.close();
  expect(s.calls.filter((call) => call.args.includes("unserve"))).toHaveLength(1);
  expect(s.removed).toEqual([MARKER]);
});
