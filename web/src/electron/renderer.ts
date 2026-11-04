import { FitAddon, Ghostty, Terminal } from "@aurigax-ai/ghostty-web";
import { DANGO_THEME, TERMINAL_FONT, startTerminalHost } from "./terminal.ts";
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
    fit: () => fit.fit(),
    size: () => ({ cols: term.cols, rows: term.rows }),
    write: (data) => term.write(data),
    focus: () => term.focus(),
    setPaused: (paused) => term.setPaused(paused),
    onData: (cb) => void term.onData(cb),
    onResize: (cb) => void term.onResize(cb),
    setClipboardWriteHandler: (handler) => {
      term.clipboardWriteHandler = handler;
    },
    dispose: () => term.dispose(),
  };
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
    pointer: { buttons: () => 0 },
    clipboard: { writeText: (text) => navigator.clipboard.writeText(text) },
    view: { showTerminal: createViewToggle(boolHidden(container), boolHidden(webuiRoot)) },
    onError: (error) => console.error(error),
  });
}

boot().catch((error: unknown) => console.error(error));
