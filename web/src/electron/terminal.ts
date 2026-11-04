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

export const TERMINAL_FONT: { fontFamily: string; fontSize: number; fontWeight: number } = {
  fontFamily: '"Maple Mono", "Symbols Nerd Font Mono", "MesloLGS Nerd Font Mono", "MesloLGS NF", monospace',
  fontSize: 13,
  fontWeight: 400, // stub: contract is 300
};

export const FONT_SIZE_MIN = 9;
export const FONT_SIZE_MAX = 28;
export const FONT_SIZE_DEFAULT = 13;
export interface KeyEventLike { key: string; code: string; shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean; isComposing: boolean }
export interface WheelEventLike { deltaY: number; deltaMode: number; clientX: number; clientY: number }

// Stubs: behaviour not implemented yet.
export function shiftEnterSequence(_event: { key: string; shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean; isComposing: boolean }): string | null {
  return null;
}
export function zoomAction(_event: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean }): "in" | "out" | "reset" | null {
  return null;
}
export function nextFontSize(current: number, _action: "in" | "out" | "reset"): number {
  return current;
}
export function createWheelAccumulator(): { push(deltaY: number, deltaMode: number, cellHeight: number): number } {
  return { push: () => 0 };
}
export function isCmdV(_event: { code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }): boolean {
  return false;
}

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
  fontSize(): number;
  setFontSize(px: number): void;
  cellHeight(): number;
  setKeyHandler(handler: (event: KeyEventLike) => boolean): void;
  setWheelHandler(handler: (event: WheelEventLike) => boolean): void;
  appOwnsWheel(): boolean;
  wheelStep(direction: "up" | "down", clientX: number, clientY: number): string;
  input(data: string): void;
  paste(text: string): void;
  dispose(): void;
}
export interface ResizeSource {
  observe(cb: () => void): () => void;
}
export interface TerminalHostDeps {
  engine: TerminalEngine;
  bridge: CpdElectronBridge;
  resize: ResizeSource;
  clipboard: { writeText(text: string): Promise<void>; readText(): Promise<string> };
  view: { showTerminal(visible: boolean): void };
  pointer: { buttons(): number };
  onError(error: unknown): void;
}

// ghostty-web 0.3.9 reports no-button motion (mode 1003) as code 32 (left drag); 35 is no-button motion.
export function fixSgrMouseHover(data: string, buttonsPressed: number): string {
  if ((buttonsPressed & 7) !== 0) return data;
  return data.replace(/\x1b\[<32;(\d+;\d+)M/g, "\x1b[<35;$1M");
}

// FitAddon.fit() ignores calls for 50 ms after one, dropping the final size; resize directly instead.
export function fitToContainer(
  addon: { proposeDimensions(): { cols: number; rows: number } | undefined },
  term: { cols: number; rows: number; resize(cols: number, rows: number): void },
): void {
  const dims = addon.proposeDimensions();
  if (!dims || (dims.cols === term.cols && dims.rows === term.rows)) return;
  term.resize(dims.cols, dims.rows);
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
  engine.onData((data) => bridge.writePty(fixSgrMouseHover(data, deps.pointer.buttons())));
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
