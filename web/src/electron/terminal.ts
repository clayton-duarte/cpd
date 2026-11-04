import type { CpdElectronBridge, PtySize } from "./channels.ts";

export const DANGO_THEME = {
  foreground: "",
  background: "",
  cursor: "",
  cursorAccent: "",
  selectionBackground: "",
  selectionForeground: "",
  black: "",
  red: "",
  green: "",
  yellow: "",
  blue: "",
  magenta: "",
  cyan: "",
  white: "",
  brightBlack: "",
  brightRed: "",
  brightGreen: "",
  brightYellow: "",
  brightBlue: "",
  brightMagenta: "",
  brightCyan: "",
  brightWhite: "",
};

export const TERMINAL_FONT: { fontFamily: string; fontSize: number } = { fontFamily: "", fontSize: 0 };

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
  onError(error: unknown): void;
}

export function startTerminalHost(_deps: TerminalHostDeps): Promise<{ dispose(): void }> {
  throw new Error("not implemented");
}
