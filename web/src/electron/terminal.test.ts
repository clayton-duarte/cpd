import type { CpdElectronBridge, PtySize, UiMode } from "./channels.ts";
import {
  DANGO_THEME,
  TERMINAL_FONT,
  fitToContainer,
  fixSgrMouseHover,
  startTerminalHost,
  type TerminalHostDeps } from "./terminal.ts";

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
  let pointerButtons = 0;

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
      sendWeb: () => {},
      onWebMessage: () => () => {},
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
    pointer: { buttons: () => pointerButtons },
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
    setButtons: (b: number) => {
      pointerButtons = b;
    },
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
  expect(h.log).toEqual(["paused false", "show true", "fit", "focus"]);
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

test("startTerminalHost fits the engine when the mode becomes tui", async () => {
  const h = harness("tui");
  await started(h);
  const startup = h.log.filter((l) => l === "fit" || l === "focus");
  expect(startup).toEqual(["fit", "fit", "focus"]);
  h.log.length = 0;
  h.fire.mode("webui");
  expect(h.log).toEqual(["paused true", "show false"]);
  h.log.length = 0;
  h.fire.mode("tui");
  expect(h.log).toEqual(["paused false", "show true", "fit", "focus"]);
});

const HOVER = "\x1b[<32;10;5M";

test("fixSgrMouseHover rewrites a no-button hover to SGR motion 35", () => {
  expect(fixSgrMouseHover(HOVER, 0)).toBe("\x1b[<35;10;5M");
  expect(fixSgrMouseHover(HOVER, 8)).toBe("\x1b[<35;10;5M");
});

test("fixSgrMouseHover rewrites every hover sequence of a chunk and keeps the rest", () => {
  expect(fixSgrMouseHover("a\x1b[<32;1;2Mb\x1b[<32;30;40Mc", 0)).toBe("a\x1b[<35;1;2Mb\x1b[<35;30;40Mc");
});

test("fixSgrMouseHover keeps a real left drag while a button is pressed", () => {
  for (const b of [1, 2, 4, 5]) expect(fixSgrMouseHover(HOVER, b)).toBe(HOVER);
});

test("fixSgrMouseHover never rewrites releases, clicks, wheel or typed text", () => {
  const same = ["\x1b[<32;10;5m", "\x1b[<0;10;5M", "\x1b[<1;10;5M", "\x1b[<2;10;5M", "\x1b[<64;10;5M", "\x1b[<33;10;5M", "\x1b[<35;10;5M", "hello 32;1;2M"];
  for (const d of same) expect(fixSgrMouseHover(d, 0)).toBe(d);
});

test("startTerminalHost sends corrected mouse data to the pty using the live pointer buttons", async () => {
  const h = harness();
  await started(h);
  h.fire.engineData(HOVER);
  h.setButtons(1);
  h.fire.engineData(HOVER);
  h.setButtons(0);
  h.fire.engineData("k");
  expect(h.sentWrites).toEqual(["\x1b[<35;10;5M", HOVER, "k"]);
});

function fitFake(dims: { cols: number; rows: number } | undefined, cols = 80, rows = 24) {
  const calls: [number, number][] = [];
  return {
    calls,
    addon: { proposeDimensions: () => dims },
    term: { cols, rows, resize: (c: number, r: number) => calls.push([c, r]) },
  };
}

test("fitToContainer resizes the terminal once to the proposed dimensions", () => {
  const f = fitFake({ cols: 100, rows: 30 });
  fitToContainer(f.addon, f.term);
  expect(f.calls).toEqual([[100, 30]]);
});

test("fitToContainer does nothing when the dimensions are unknown or unchanged", () => {
  const unknown = fitFake(undefined);
  fitToContainer(unknown.addon, unknown.term);
  const same = fitFake({ cols: 80, rows: 24 });
  fitToContainer(same.addon, same.term);
  expect(unknown.calls).toEqual([]);
  expect(same.calls).toEqual([]);
});

test("TERMINAL_FONT falls back to the Nerd Font stack", () => {
  expect(TERMINAL_FONT).toEqual({
    fontFamily: '"Maple Mono", "Symbols Nerd Font Mono", "MesloLGS Nerd Font Mono", "MesloLGS NF", monospace',
    fontSize: 13,
  });
});
