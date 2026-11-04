import { FitAddon, Ghostty, Terminal } from "@aurigax-ai/ghostty-web";
import { DANGO_THEME, TERMINAL_FONT, fitToContainer, startTerminalHost } from "./terminal.ts";
import type { TerminalEngine } from "./terminal.ts";
import { createViewToggle } from "./view.ts";
import { mountWebui } from "./webui.tsx";

function element(id: string): HTMLElement {
  const found = document.getElementById(id);
  if (!found) throw new Error("missing #" + id);
  return found;
}

// HTMLElement.hidden is typed string | boolean; the toggle only writes booleans.
function boolHidden(el: HTMLElement): { hidden: boolean } {
  return {
    get hidden() { return el.hidden === true; },
    set hidden(value: boolean) { el.hidden = value; },
  } as { hidden: boolean };
}

let buttons = 0;
for (const type of ["mousedown", "mouseup", "mousemove"] as const) {
  window.addEventListener(type, (e: MouseEvent) => { buttons = e.buttons; }, true);
}

async function boot(): Promise<void> {
  const bridge = window.cpdElectron;
  const container = element("terminal");
  const webuiRoot = element("webui-root");
  mountWebui(webuiRoot, bridge);
  const ghostty = await Ghostty.fromBytes(await bridge.readWasm());
  const term = new Terminal({ ghostty, renderer: "webgl", theme: DANGO_THEME, ...TERMINAL_FONT });
  const fit = new FitAddon();
  term.loadAddon(fit);
  const engine: TerminalEngine = {
    open: () => term.open(container),
    fit: () => fitToContainer(fit, term),
    size: () => ({ cols: term.cols, rows: term.rows }),
    write: (data) => term.write(data),
    focus: () => term.focus(),
    setPaused: (paused) => term.setPaused(paused),
    onData: (cb) => void term.onData(cb),
    onResize: (cb) => void term.onResize(cb),
    setClipboardWriteHandler: (handler) => {
      term.clipboardWriteHandler = handler;
    },
    fontSize: () => term.options.fontSize,
    setFontSize: (px) => { term.options.fontSize = px; },
    cellHeight: () => term.renderer?.getMetrics().height ?? 20,
    setKeyHandler: (handler) => term.attachCustomKeyEventHandler(handler),
    setWheelHandler: (handler) => term.attachCustomWheelEventHandler(handler),
    appOwnsWheel: () => term.hasMouseTracking() || (term.wasmTerm?.isAlternateScreen() ?? false),
    wheelStep: (direction, clientX, clientY) => {
      if (!term.hasMouseTracking()) return direction === "up" ? "\x1b[A" : "\x1b[B";
      const metrics = term.renderer?.getMetrics();
      const rect = container.getBoundingClientRect();
      const col = Math.max(1, Math.floor((clientX - rect.left) / (metrics?.width || 1)) + 1);
      const row = Math.max(1, Math.floor((clientY - rect.top) / (metrics?.height || 1)) + 1);
      return "\x1b[<" + (direction === "up" ? 64 : 65) + ";" + col + ";" + row + "M";
    },
    input: (data) => term.input(data, true),
    paste: (text) => term.paste(text),
    dispose: () => term.dispose(),
  };
  await document.fonts?.ready;
  await startTerminalHost({
    engine,
    bridge,
    resize: {
      observe: (cb) => {
        const observer = new ResizeObserver(() => cb());
        observer.observe(container);
        return () => observer.disconnect();
      },
    },
    pointer: { buttons: () => buttons },
    clipboard: { writeText: (text) => navigator.clipboard.writeText(text), readText: () => navigator.clipboard.readText() },
    view: { showTerminal: createViewToggle(boolHidden(container), boolHidden(webuiRoot)) },
    onError: (error) => console.error(error),
  });
}

boot().catch((error: unknown) => console.error(error));
