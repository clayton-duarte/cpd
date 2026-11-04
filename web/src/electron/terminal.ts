import type { CpdElectronBridge, PtySize, UiMode } from "./channels.ts";

export const DANGO_THEME = {
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
};

export const TERMINAL_FONT: { fontFamily: string; fontSize: number } = {
  fontFamily: '"Maple Mono", monospace',
  fontSize: 13,
};

export interface TerminalEngine {
  open(): void;
  fit(): void;
  size(): PtySize;
  write(data: string): void;
  focus(): void;
  setPaused(paused: boolean): void;
  onData(cb: (data: string) => void): void;
  onResize(cb: (size: PtySize) => void): void;
  setClipboardWriteHandler(handler: (text: string) => boolean): void;
  dispose(): void;
}
export interface ResizeSource {
  observe(cb: () => void): () => void;
}
export interface TerminalHostDeps {
  engine: TerminalEngine;
  bridge: CpdElectronBridge;
  resize: ResizeSource;
  clipboard: { writeText(text: string): Promise<void> };
  view: { showTerminal(visible: boolean): void };
  pointer: { buttons(): number };
  onError(error: unknown): void;
}

export function fixSgrMouseHover(_data: string, _buttonsPressed: number): string {
  throw new Error("not implemented");
}

export function fitToContainer(
  _addon: { proposeDimensions(): { cols: number; rows: number } | undefined },
  _term: { cols: number; rows: number; resize(cols: number, rows: number): void },
): void {
  throw new Error("not implemented");
}

export async function startTerminalHost(deps: TerminalHostDeps): Promise<{ dispose(): void }> {
  const { engine, bridge } = deps;
  engine.open();
  engine.fit();
  const unsubscribes: (() => void)[] = [];
  unsubscribes.push(bridge.onPtyData((data) => engine.write(data)));
  unsubscribes.push(
    bridge.onPtyExit((code) => engine.write("\r\n[cpd tui exited with code " + code + "]\r\n")),
  );
  engine.onData((data) => bridge.writePty(data));
  unsubscribes.push(deps.resize.observe(() => engine.fit()));
  engine.setClipboardWriteHandler((text) => {
    deps.clipboard.writeText(text).catch(deps.onError);
    return true;
  });
  const apply = (mode: UiMode): void => {
    engine.setPaused(mode !== "tui");
    deps.view.showTerminal(mode === "tui");
    if (mode === "tui") {
      engine.fit();
      engine.focus();
    }
  };
  apply(await bridge.getMode());
  unsubscribes.push(bridge.onMode(apply));
  await bridge.startPty(engine.size());
  engine.onResize((size) => bridge.resizePty(size));
  return {
    dispose() {
      for (const off of unsubscribes) off();
      engine.dispose();
    },
  };
}
