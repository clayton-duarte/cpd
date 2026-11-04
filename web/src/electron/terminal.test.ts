import type { CpdElectronBridge, PtySize, UiMode } from "./channels.ts";
import {
  DANGO_THEME,
  FONT_SIZE_DEFAULT,
  FONT_SIZE_MAX,
  FONT_SIZE_MIN,
  createWheelAccumulator,
  isCmdV,
  nextFontSize,
  shiftEnterSequence,
  zoomAction,
  type KeyEventLike,
  type WheelEventLike,
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
  let keyHandler: (e: KeyEventLike) => boolean = () => undefined as unknown as boolean; // unregistered
  let wheelHandler: (e: WheelEventLike) => boolean = () => undefined as unknown as boolean; // unregistered
  let fontSizePx = 13;
  let ownsWheel = false;
  let clipboardRead: Promise<string> = Promise.resolve("");
  const pasted: string[] = [];
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
      fontSize: () => fontSizePx,
      setFontSize: (px) => {
        log.push(`setFontSize ${px}`);
        fontSizePx = px;
      },
      cellHeight: () => 16,
      setKeyHandler: (h) => {
        keyHandler = h;
      },
      setWheelHandler: (h) => {
        wheelHandler = h;
      },
      appOwnsWheel: () => ownsWheel,
      wheelStep: (dir, x, y) => (dir === "up" ? `<U${x},${y}>` : `<D${x},${y}>`),
      input: (d) => engineData(d),
      paste: (t) => {
        pasted.push(t);
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
      readText: () => clipboardRead,
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
    pasted,
    key: (e: KeyEventLike) => keyHandler(e),
    wheel: (e: WheelEventLike) => wheelHandler(e),
    setOwnsWheel: (v: boolean) => {
      ownsWheel = v;
    },
    setClipboardRead: (p: Promise<string>) => {
      clipboardRead = p;
    },
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
    fontWeight: 300,
  });
});

const key = (over: Partial<KeyEventLike> = {}): KeyEventLike => ({
  key: "",
  code: "",
  shiftKey: false,
  ctrlKey: false,
  altKey: false,
  metaKey: false,
  isComposing: false,
  ...over,
});

test("shiftEnterSequence returns the CSI-u sequence for plain Shift Enter", () => {
  expect(shiftEnterSequence(key({ key: "Enter", shiftKey: true }))).toBe("\x1b[13;2u");
});

test("shiftEnterSequence ignores other modifiers and composition", () => {
  expect(shiftEnterSequence(key({ key: "Enter", shiftKey: true }))).toBe("\x1b[13;2u");
  expect(shiftEnterSequence(key({ key: "Enter" }))).toBeNull();
  expect(shiftEnterSequence(key({ key: "Enter", shiftKey: true, ctrlKey: true }))).toBeNull();
  expect(shiftEnterSequence(key({ key: "Enter", shiftKey: true, altKey: true }))).toBeNull();
  expect(shiftEnterSequence(key({ key: "Enter", shiftKey: true, metaKey: true }))).toBeNull();
  expect(shiftEnterSequence(key({ key: "Enter", shiftKey: true, isComposing: true }))).toBeNull();
  expect(shiftEnterSequence(key({ key: "a", shiftKey: true }))).toBeNull();
});

test("zoomAction maps Cmd equals Cmd plus Cmd minus and Cmd zero", () => {
  const z = (k: string, shiftKey = false) => zoomAction(key({ key: k, metaKey: true, shiftKey }));
  expect(z("=")).toBe("in");
  expect(z("+", true)).toBe("in");
  expect(z("-")).toBe("out");
  expect(z("_", true)).toBe("out");
  expect(z("0")).toBe("reset");
  expect(z("a")).toBeNull();
});

test("zoomAction ignores events without Cmd", () => {
  expect(zoomAction(key({ key: "=", metaKey: true }))).toBe("in");
  expect(zoomAction(key({ key: "=" }))).toBeNull();
  expect(zoomAction(key({ key: "=", ctrlKey: true }))).toBeNull();
  expect(zoomAction(key({ key: "=", metaKey: true, ctrlKey: true }))).toBeNull();
  expect(zoomAction(key({ key: "-", metaKey: true, altKey: true }))).toBeNull();
});

test("nextFontSize steps by one and clamps to the range", () => {
  expect(nextFontSize(13, "in")).toBe(14);
  expect(nextFontSize(13, "out")).toBe(12);
  expect(nextFontSize(FONT_SIZE_MAX, "in")).toBe(FONT_SIZE_MAX);
  expect(nextFontSize(FONT_SIZE_MIN, "out")).toBe(FONT_SIZE_MIN);
});

test("nextFontSize reset returns the default size", () => {
  expect(FONT_SIZE_DEFAULT).toBe(13);
  expect(nextFontSize(20, "reset")).toBe(13);
  expect(nextFontSize(9, "reset")).toBe(13);
});

test("wheel accumulator emits no step below the threshold", () => {
  const w = createWheelAccumulator();
  expect(w.push(10, 0, 16)).toBe(0);
  expect(w.push(10, 0, 16)).toBe(0);
  expect(w.push(10, 0, 16)).toBe(1);
});

test("wheel accumulator emits one step per threshold and keeps the remainder", () => {
  const w = createWheelAccumulator();
  expect(w.push(30, 0, 16)).toBe(1);
  expect(w.push(20, 0, 16)).toBe(1);
  expect(w.push(-0, 0, 16)).toBe(0);
  const big = createWheelAccumulator();
  expect(big.push(-100, 0, 16)).toBe(-4);
  expect(big.push(-20, 0, 16)).toBe(-1);
  const tall = createWheelAccumulator();
  expect(tall.push(30, 0, 40)).toBe(0);
  expect(tall.push(30, 0, 40)).toBe(1);
});

test("wheel accumulator zeroes the remainder on direction reversal", () => {
  const w = createWheelAccumulator();
  expect(w.push(20, 0, 16)).toBe(0);
  expect(w.push(-20, 0, 16)).toBe(0);
  expect(w.push(-10, 0, 16)).toBe(-1);
});

test("wheel accumulator converts line deltas to pixels", () => {
  const w = createWheelAccumulator();
  expect(w.push(3, 1, 16)).toBe(2);
  const pages = createWheelAccumulator();
  expect(pages.push(1, 2, 16)).toBe(6);
});

test("isCmdV matches only Cmd V", () => {
  const v = (over: object) => isCmdV({ code: "KeyV", metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, ...over });
  expect(v({})).toBe(true);
  expect(v({ metaKey: false })).toBe(false);
  expect(v({ ctrlKey: true })).toBe(false);
  expect(v({ altKey: true })).toBe(false);
  expect(v({ shiftKey: true })).toBe(false);
  expect(v({ code: "KeyC" })).toBe(false);
});

test("host writes CSI-u to the pty on Shift Enter and consumes the key", async () => {
  const h = harness();
  await started(h);
  expect(h.key(key({ key: "Enter", shiftKey: true }))).toBe(true);
  expect(h.sentWrites).toEqual(["\x1b[13;2u"]);
  expect(h.key(key({ key: "Enter" }))).toBe(false);
  expect(h.sentWrites).toEqual(["\x1b[13;2u"]);
});

test("host zooms the font and refits the terminal on zoom keys", async () => {
  const h = harness();
  await started(h);
  h.log.length = 0;
  expect(h.key(key({ key: "=", metaKey: true }))).toBe(true);
  expect(h.log).toEqual(["setFontSize 14", "fit"]);
  expect(h.key(key({ key: "0", metaKey: true }))).toBe(true);
  expect(h.log.slice(2)).toEqual(["setFontSize 13", "fit"]);
});

test("host pastes the clipboard text on Cmd V", async () => {
  const h = harness();
  await started(h);
  h.setClipboardRead(Promise.resolve("hello"));
  expect(h.key(key({ code: "KeyV", key: "v", metaKey: true }))).toBe(true);
  await h.flush();
  expect(h.pasted).toEqual(["hello"]);
  h.setClipboardRead(Promise.resolve(""));
  expect(h.key(key({ code: "KeyV", key: "v", metaKey: true }))).toBe(true);
  await h.flush();
  expect(h.pasted).toEqual(["hello"]);
});

test("host consumes wheel events and writes one step per threshold when the app owns the wheel", async () => {
  const h = harness();
  await started(h);
  h.setOwnsWheel(true);
  expect(h.wheel({ deltaY: 10, deltaMode: 0, clientX: 5, clientY: 6 })).toBe(true);
  expect(h.sentWrites).toEqual([]);
  expect(h.wheel({ deltaY: 40, deltaMode: 0, clientX: 5, clientY: 6 })).toBe(true);
  expect(h.sentWrites).toEqual(["<D5,6>".repeat(2)]);
  expect(h.wheel({ deltaY: -100, deltaMode: 0, clientX: 1, clientY: 2 })).toBe(true);
  expect(h.sentWrites).toEqual(["<D5,6>".repeat(2), "<U1,2>".repeat(4)]);
});

test("host leaves wheel events to the engine when the app does not own the wheel", async () => {
  const h = harness();
  await started(h);
  h.setOwnsWheel(false);
  expect(h.wheel({ deltaY: 100, deltaMode: 0, clientX: 5, clientY: 6 })).toBe(false);
  expect(h.sentWrites).toEqual([]);
});
