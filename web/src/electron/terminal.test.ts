import type { CpdElectronBridge, PtySize, UiMode } from "./channels.ts";
import { DANGO_THEME, TERMINAL_FONT, startTerminalHost, type TerminalHostDeps } from "./terminal.ts";

function harness(mode: UiMode = "tui") {
  const log: string[] = [];
  const errors: unknown[] = [];
  const clipboardTexts: string[] = [];
  let ptyData: (d: string) => void = () => {};
  let ptyExit: (c: number) => void = () => {};
  let modeCb: (m: UiMode) => void = () => {};
  let engineData: (d: string) => void = () => {};
  let engineResize: (s: PtySize) => void = () => {};
  let observed: () => void = () => {};
  let clipboardHandler: (t: string) => boolean = () => false;
  let resolveStart: () => void = () => {};
  let rejectWrite: { error: unknown } | undefined;
  let engineSize: PtySize = { cols: 80, rows: 24 };
  const writes: string[] = [];
  const sentWrites: string[] = [];
  const resizes: PtySize[] = [];

  const deps: TerminalHostDeps = {
    engine: {
      open: () => log.push("open"),
      fit: () => log.push("fit"),
      size: () => {
        log.push("size");
        return engineSize;
      },
      write: (d) => {
        log.push("engine.write");
        writes.push(d);
      },
      focus: () => log.push("focus"),
      setPaused: (p) => log.push(`paused ${p}`),
      onData: (cb) => {
        log.push("engine.onData");
        engineData = cb;
      },
      onResize: (cb) => {
        log.push("engine.onResize");
        engineResize = cb;
      },
      setClipboardWriteHandler: (h) => {
        log.push("clipboardHandler");
        clipboardHandler = h;
      },
      dispose: () => log.push("engine.dispose"),
    },
    bridge: {
      readWasm: () => Promise.resolve(new Uint8Array()),
      getMode: () => {
        log.push("getMode");
        return Promise.resolve(mode);
      },
      startPty: (size) => {
        log.push(`startPty ${size.cols}x${size.rows}`);
        return new Promise<void>((resolve) => {
          resolveStart = resolve;
        });
      },
      writePty: (d) => {
        log.push("writePty");
        sentWrites.push(d);
      },
      resizePty: (s) => {
        log.push("resizePty");
        resizes.push(s);
      },
      onPtyData: (cb) => {
        log.push("onPtyData");
        ptyData = cb;
        return () => log.push("off onPtyData");
      },
      onPtyExit: (cb) => {
        log.push("onPtyExit");
        ptyExit = cb;
        return () => log.push("off onPtyExit");
      },
      onMode: (cb) => {
        log.push("onMode");
        modeCb = cb;
        return () => log.push("off onMode");
      },
    } satisfies CpdElectronBridge,
    resize: {
      observe: (cb) => {
        log.push("observe");
        observed = cb;
        return () => log.push("unobserve");
      },
    },
    clipboard: {
      writeText: (t) => {
        clipboardTexts.push(t);
        return rejectWrite === undefined ? Promise.resolve() : Promise.reject(rejectWrite.error);
      },
    },
    view: { showTerminal: (v) => log.push(`show ${v}`) },
    onError: (e) => errors.push(e),
  };
  const flush = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
  };
  return {
    deps,
    log,
    errors,
    writes,
    sentWrites,
    resizes,
    clipboardTexts,
    flush,
    resolveStart: () => resolveStart(),
    setEngineSize: (s: PtySize) => {
      engineSize = s;
    },
    failClipboard: (e: unknown) => {
      rejectWrite = { error: e };
    },
    fire: {
      ptyData: (d: string) => ptyData(d),
      ptyExit: (c: number) => ptyExit(c),
      mode: (m: UiMode) => modeCb(m),
      engineData: (d: string) => engineData(d),
      engineResize: (s: PtySize) => engineResize(s),
      observed: () => observed(),
      clipboard: (t: string) => clipboardHandler(t),
    },
  };
}

/** Starts the host and lets it reach `await startPty`; returns the pending promise. */
async function started(h: ReturnType<typeof harness>) {
  const promise = startTerminalHost(h.deps);
  await h.flush();
  h.resolveStart();
  const host = await promise;
  return host;
}

test("DANGO_THEME is the Dango v3 palette", () => {
  expect(DANGO_THEME).toEqual({
    foreground: "#d8d2cc",
    background: "#1c1a20",
    cursor: "#ecaec7",
    cursorAccent: "#1c1a20",
    selectionBackground: "#5a504a",
    selectionForeground: "#d8d2cc",
    black: "#5a504a",
    red: "#e17f6e",
    green: "#8da96e",
    yellow: "#c29653",
    blue: "#7aa2ce",
    magenta: "#ca88a3",
    cyan: "#67ada7",
    white: "#aba3af",
    brightBlack: "#87808c",
    brightRed: "#ffa796",
    brightGreen: "#b3cd96",
    brightYellow: "#e4bb7f",
    brightBlue: "#a1c6ef",
    brightMagenta: "#ecaec7",
    brightCyan: "#92d1ca",
    brightWhite: "#f3ead9",
  });
});

test("TERMINAL_FONT is Maple Mono at 13px", () => {
  expect(TERMINAL_FONT).toEqual({ fontFamily: '"Maple Mono", monospace', fontSize: 13 });
});

test("startTerminalHost opens, fits, wires data both ways and starts the pty at the fitted size", async () => {
  const h = harness();
  h.setEngineSize({ cols: 120, rows: 40 });
  await started(h);
  expect(h.log.slice(0, 2)).toEqual(["open", "fit"]);
  expect(h.log).toContain("startPty 120x40");
  h.fire.ptyData("out");
  expect(h.writes).toEqual(["out"]);
  h.fire.engineData("key");
  expect(h.sentWrites).toEqual(["key"]);
});

test("startTerminalHost registers every listener before startPty", async () => {
  const h = harness();
  await started(h);
  const start = h.log.findIndex((l) => l.startsWith("startPty"));
  expect(start).toBeGreaterThan(-1);
  for (const name of ["onPtyData", "onPtyExit", "engine.onData", "observe", "clipboardHandler", "onMode"]) {
    const i = h.log.indexOf(name);
    expect(i, name).toBeGreaterThan(-1);
    expect(i, name).toBeLessThan(start);
  }
  expect(h.log.indexOf("engine.onResize")).toBeGreaterThan(start);
  expect(h.log.indexOf("fit")).toBeLessThan(h.log.indexOf("onPtyData"));
});

test("startTerminalHost does not forward resizes before the pty started", async () => {
  const h = harness();
  const promise = startTerminalHost(h.deps);
  await h.flush();
  expect(h.log).toContain("startPty 80x24");
  expect(h.log).not.toContain("engine.onResize");
  h.resolveStart();
  await promise;
  expect(h.log).toContain("engine.onResize");
  expect(h.resizes).toEqual([]);
  h.fire.engineResize({ cols: 90, rows: 30 });
  expect(h.resizes).toEqual([{ cols: 90, rows: 30 }]);
});

test("startTerminalHost applies the mode from the bridge and later changes", async () => {
  const h = harness("webui");
  await started(h);
  const initial = h.log.filter((l) => l.startsWith("paused") || l.startsWith("show") || l === "focus");
  expect(initial).toEqual(["paused true", "show false"]);
  h.log.length = 0;
  h.fire.mode("tui");
  expect(h.log).toEqual(["paused false", "show true", "focus"]);
  h.log.length = 0;
  h.fire.mode("webui");
  expect(h.log).toEqual(["paused true", "show false"]);
});

test("startTerminalHost OSC 52 writes go to the clipboard and return true", async () => {
  const h = harness();
  await started(h);
  expect(h.fire.clipboard("copied")).toBe(true);
  expect(h.clipboardTexts).toEqual(["copied"]);
  await h.flush();
  expect(h.errors).toEqual([]);

  const boom = new Error("clipboard denied");
  h.failClipboard(boom);
  expect(h.fire.clipboard("again")).toBe(true);
  await h.flush();
  expect(h.errors).toEqual([boom]);
});

test("startTerminalHost refits when the container resizes", async () => {
  const h = harness();
  await started(h);
  h.log.length = 0;
  h.fire.observed();
  expect(h.log).toEqual(["fit"]);
});

test("startTerminalHost dispose unsubscribes everything", async () => {
  const h = harness();
  const host = await started(h);
  h.log.length = 0;
  host.dispose();
  expect([...h.log].sort()).toEqual(["engine.dispose", "off onMode", "off onPtyData", "off onPtyExit", "unobserve"]);
});

test("startTerminalHost prints the exit code when the pty exits", async () => {
  const h = harness();
  await started(h);
  h.fire.ptyExit(7);
  expect(h.writes).toEqual(["\r\n[cpd tui exited with code 7]\r\n"]);
});
