import path from "node:path";
import type { UiMode } from "./channels.ts";
import type { MenuTemplate } from "./menu.ts";
import {
  createModeController,
  modeFilePath,
  parseUiMode,
  readMode,
  writeMode,
  type ModeFs,
} from "./mode.ts";

function fakeFs(initial: Record<string, string> = {}, log: string[] = []) {
  const files = new Map(Object.entries(initial));
  const state = { writeError: undefined as unknown, readError: undefined as unknown };
  const fs: ModeFs = {
    readFile(p) {
      log.push(`read ${p}`);
      if (state.readError !== undefined) return Promise.reject(state.readError);
      const data = files.get(p);
      if (data === undefined) return Promise.reject(Object.assign(new Error("missing"), { code: "ENOENT" }));
      return Promise.resolve(data);
    },
    writeFile(p, data) {
      log.push(`write ${p} ${data}`);
      if (state.writeError !== undefined) return Promise.reject(state.writeError);
      files.set(p, data);
      return Promise.resolve();
    },
    mkdir(p) {
      log.push(`mkdir ${p}`);
      return Promise.resolve();
    },
  };
  return { fs, files, state };
}

function harness(initial: Record<string, string> = {}, platform: NodeJS.Platform = "linux") {
  const log: string[] = [];
  const templates: MenuTemplate[] = [];
  const errors: unknown[] = [];
  const f = fakeFs(initial, log);
  const controller = createModeController({
    fs: f.fs,
    file: "/h/run/ui-mode.json",
    platform,
    menu: {
      setApplicationMenu(t) {
        templates.push(t);
        const checked = ((t.find((i) => i.label === "View")?.submenu ?? []) as { checked?: boolean }[]).map((i) => i.checked);
        log.push(`menu ${checked.join(",")}`);
      },
    },
    window: {
      setIgnoreMenuShortcuts: (ignore) => log.push(`ignore ${ignore}`),
      sendMode: (mode) => log.push(`send ${mode}`),
    },
    onError: (e) => errors.push(e),
  });
  return { controller, log, templates, errors, ...f };
}

async function flush() {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

const FILE = "/h/run/ui-mode.json";

test("parseUiMode accepts tui and webui only", () => {
  expect(parseUiMode("tui")).toBe("tui");
  expect(parseUiMode("webui")).toBe("webui");
  expect(() => parseUiMode("TUI")).toThrow('invalid ui mode: "TUI"');
  expect(() => parseUiMode(undefined)).toThrow("invalid ui mode: ");
  expect(() => parseUiMode(1)).toThrow("invalid ui mode: 1");
});

test("modeFilePath is run/ui-mode.json under CPD_HOME", () => {
  expect(modeFilePath("/home/u/.cpd")).toBe(path.join("/home/u/.cpd", "run", "ui-mode.json"));
});

test("readMode returns tui when the file is missing", async () => {
  await expect(readMode(fakeFs().fs, FILE)).resolves.toBe("tui");
});

test("readMode rejects invalid content naming the file", async () => {
  await expect(readMode(fakeFs({ [FILE]: "not json" }).fs, FILE)).rejects.toThrow(FILE);
  await expect(readMode(fakeFs({ [FILE]: "42" }).fs, FILE)).rejects.toThrow(FILE);
  await expect(readMode(fakeFs({ [FILE]: "null" }).fs, FILE)).rejects.toThrow(FILE);
  await expect(readMode(fakeFs({ [FILE]: '{"mode":"x"}' }).fs, FILE)).rejects.toThrow(FILE);
  await expect(readMode(fakeFs({ [FILE]: "{}" }).fs, FILE)).rejects.toThrow(FILE);
  await expect(readMode(fakeFs({ [FILE]: '{"mode":"webui"}' }).fs, FILE)).resolves.toBe("webui");
});

test("readMode propagates other fs errors", async () => {
  const f = fakeFs();
  const boom = Object.assign(new Error("denied"), { code: "EACCES" });
  f.state.readError = boom;
  await expect(readMode(f.fs, FILE)).rejects.toBe(boom);
});

test("writeMode creates the directory then writes the mode json", async () => {
  const log: string[] = [];
  const f = fakeFs({}, log);
  await writeMode(f.fs, FILE, "webui");
  expect(log).toEqual([`mkdir ${path.dirname(FILE)}`, `write ${FILE} {"mode":"webui"}\n`]);
});

test("controller init applies the persisted mode", async () => {
  const h = harness({ [FILE]: '{"mode":"webui"}' });
  expect(h.controller.getMode()).toBe("tui");
  await h.controller.init();
  expect(h.controller.getMode()).toBe("webui");
  expect(h.log).toEqual([`read ${FILE}`, "menu false,true", "ignore false", "send webui"]);
});

test("controller setMode persists then applies menu, shortcuts and window in order", async () => {
  const h = harness();
  await h.controller.setMode("webui");
  expect(h.controller.getMode()).toBe("webui");
  expect(h.log).toEqual([
    `mkdir ${path.dirname(FILE)}`,
    `write ${FILE} {"mode":"webui"}\n`,
    "menu false,true",
    "ignore false",
    "send webui",
  ]);
});

test("controller setMode to the current mode does nothing", async () => {
  const h = harness();
  await h.controller.setMode("tui");
  expect(h.log).toEqual([]);
  expect(h.controller.getMode()).toBe("tui");
});

test("controller setMode keeps the old mode when the write fails", async () => {
  const h = harness();
  const boom = new Error("disk full");
  h.state.writeError = boom;
  await expect(h.controller.setMode("webui")).rejects.toBe(boom);
  expect(h.controller.getMode()).toBe("tui");
  expect(h.templates).toEqual([]);
  expect(h.log.some((l) => l.startsWith("menu") || l.startsWith("send") || l.startsWith("ignore"))).toBe(false);
});

test("menu click selects the mode through the controller", async () => {
  const h = harness({ [FILE]: '{"mode":"tui"}' }, "darwin");
  await h.controller.init();
  const view = h.templates[0]?.find((i) => i.label === "View");
  const items = (view?.submenu ?? []) as { label?: string; click?: () => void }[];
  items.find((i) => i.label === "WebUI")?.click?.();
  await flush();
  expect(h.errors).toEqual([]);
  expect(h.files.get(FILE)).toBe('{"mode":"webui"}\n');

  h.state.writeError = new Error("nope");
  const latest = h.templates[h.templates.length - 1]?.find((i) => i.label === "View");
  const back = ((latest?.submenu ?? []) as { label?: string; click?: () => void }[]).find((i) => i.label === "TUI");
  back?.click?.();
  await flush();
  expect(h.errors).toHaveLength(1);
  expect(h.controller.getMode() satisfies UiMode).toBe("webui");
});
