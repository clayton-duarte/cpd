import path from "node:path";
import type { UiMode } from "./channels.ts";
import type { MenuTemplate } from "./menu.ts";
import { createModeController, parseUiMode, LAUNCH_MODE, type ModeFs } from "./mode.ts";

const LEGACY = "/h/run/ui-mode.json";
const VIEW = "/h/run/ui-view.json";
const PID = 4242;

// The dropped ui-mode.json is never read: the fs surface has no read at all (air-d40).
function fakeFs(initial: Record<string, string> = {}, log: string[] = [], writes: Write[] = []) {
  const files = new Map(Object.entries(initial));
  const state = { writeError: undefined as unknown, removeError: undefined as unknown };
  const fs: ModeFs = {
    writeFile(p, data) {
      log.push(`write ${p} ${data}`);
      if (state.writeError !== undefined) return Promise.reject(state.writeError);
      writes.push({ path: p, data });
      files.set(p, data);
      return Promise.resolve();
    },
    remove(p) {
      log.push(`remove ${p}`);
      if (state.removeError !== undefined) return Promise.reject(state.removeError);
      if (!files.has(p)) return Promise.reject(Object.assign(new Error("missing"), { code: "ENOENT" }));
      files.delete(p);
      return Promise.resolve();
    },
    mkdir(p) {
      log.push(`mkdir ${p}`);
      return Promise.resolve();
    },
  };
  return { fs, files, state };
}

interface Write {
  path: string;
  data: string;
}

function viewItems(template: MenuTemplate) {
  const view = template.find((item) => item.label === "View");
  return (view?.submenu ?? []) as { label?: string; checked?: boolean }[];
}

function harness(initial: Record<string, string> = {}, platform: NodeJS.Platform = "linux") {
  const log: string[] = [];
  const writes: Write[] = [];
  const templates: MenuTemplate[] = [];
  const errors: unknown[] = [];
  const f = fakeFs(initial, log, writes);
  const controller = createModeController({
    fs: f.fs,
    legacyFile: LEGACY,
    viewFile: VIEW,
    pid: PID,
    platform,
    log: (line) => log.push(`log ${line}`),
    menu: {
      setApplicationMenu(t) {
        templates.push(t);
        const checked = viewItems(t).map((i) => i.checked);
        log.push(`menu ${checked.join(",")}`);
      },
    },
    window: {
      setIgnoreMenuShortcuts: (ignore) => log.push(`ignore ${ignore}`),
      sendMode: (mode) => log.push(`send ${mode}`),
    },
    onError: (e) => errors.push(e),
  });
  return { controller, log, writes, templates, errors, ...f };
}

async function flush() {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

function markerWrites(h: ReturnType<typeof harness>) {
  return h.writes.filter((w) => w.path === VIEW).map((w) => JSON.parse(w.data));
}

test("parseUiMode accepts tui, webui and webui-preview only", () => {
  expect(parseUiMode("tui")).toBe("tui");
  expect(parseUiMode("webui")).toBe("webui");
  expect(parseUiMode("webui-preview")).toBe("webui-preview");
  expect(() => parseUiMode("TUI")).toThrow('invalid ui mode: "TUI"');
  expect(() => parseUiMode(undefined)).toThrow("invalid ui mode: ");
  expect(() => parseUiMode(1)).toThrow("invalid ui mode: 1");
});

test("launches in webui and never reads ui-mode.json", async () => {
  const h = harness({ [LEGACY]: '{"mode":"tui"}' });
  expect(h.controller.getMode()).toBe(LAUNCH_MODE);
  await h.controller.init();
  expect(h.controller.getMode()).toBe("webui");
  expect(h.log).toContain("menu false,true,false");
  expect(h.log).toContain("ignore false");
  expect(h.log).toContain("send webui");
  // No launch view is ever read back from a file.
  expect(h.log.some((l) => l.startsWith("read "))).toBe(false);
});

test("deletes the stale ui-mode.json once and logs it", async () => {
  const withFile = harness({ [LEGACY]: '{"mode":"tui"}' });
  await withFile.controller.init();
  expect(withFile.log.filter((l) => l.startsWith("remove "))).toEqual([`remove ${LEGACY}`]);
  const lines = withFile.log.filter((l) => l.startsWith("log ") && l.includes(LEGACY));
  expect(lines).toHaveLength(1);
  expect(withFile.files.has(LEGACY)).toBe(false);

  const withoutFile = harness();
  await expect(withoutFile.controller.init()).resolves.toBeUndefined();
  expect(withoutFile.log.filter((l) => l.startsWith("log "))).toEqual([]);
});

test("writes the ui-view marker on launch and on every setMode", async () => {
  const h = harness();
  await h.controller.init();
  await h.controller.setMode("tui");
  await h.controller.setMode("webui-preview");
  expect(markerWrites(h)).toEqual([
    { view: "webui", pid: PID },
    { view: "tui", pid: PID },
    { view: "webui-preview", pid: PID },
  ]);
  expect(h.log).toContain(`mkdir ${path.dirname(VIEW)}`);
  expect(h.controller.getMode()).toBe("webui-preview");
});

test("removes the ui-view marker on graceful quit", async () => {
  const h = harness();
  await h.controller.init();
  await h.controller.setMode("tui");
  await h.controller.dispose();
  expect(h.log.filter((l) => l === `remove ${VIEW}`)).toEqual([`remove ${VIEW}`]);
  expect(h.files.has(VIEW)).toBe(false);
});

test("treats webui-preview as webui for the renderer channel", async () => {
  const h = harness();
  await h.controller.init();
  await h.controller.setMode("webui-preview");
  expect(h.controller.getMode()).toBe("webui-preview");
  expect(h.log).toContain("menu false,false,true");
  expect(h.log).toContain("ignore false");
  expect(h.log).toContain("send webui");
  expect(h.log).not.toContain("send webui-preview");
});

test("setMode applies menu, shortcuts and the renderer channel in order", async () => {
  const h = harness();
  await h.controller.init();
  const before = h.log.length;
  await h.controller.setMode("tui");
  const tail = h.log.slice(before);
  const step = (line: string) => tail.indexOf(line);
  expect(step("menu true,false,false")).toBeGreaterThanOrEqual(0);
  expect(step("menu true,false,false")).toBeLessThan(step("ignore true"));
  expect(step("ignore true")).toBeLessThan(step("send tui"));
  expect(JSON.parse(h.writes.at(-1)!.data)).toEqual({ view: "tui", pid: PID });
});

test("setMode to the current view applies nothing", async () => {
  const h = harness();
  await h.controller.init();
  const before = h.log.length;
  await h.controller.setMode("webui");
  expect(h.log.slice(before).some((l) => l.startsWith("menu") || l.startsWith("send") || l.startsWith("ignore"))).toBe(false);
  expect(h.controller.getMode()).toBe("webui");
});

test("menu click selects the mode through the controller", async () => {
  const h = harness({}, "darwin");
  await h.controller.init();
  const view = h.templates[0]?.find((i) => i.label === "View");
  const items = (view?.submenu ?? []) as { label?: string; click?: () => void }[];
  items.find((i) => i.label === "WebUI Preview")?.click?.();
  await flush();
  expect(h.errors).toEqual([]);
  expect(h.controller.getMode() satisfies UiMode).toBe("webui-preview");
  expect(markerWrites(h).at(-1)).toEqual({ view: "webui-preview", pid: PID });
});
