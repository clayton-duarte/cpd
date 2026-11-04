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
  fontWeight: 300,
};

export const FONT_SIZE_MIN = 9;
export const FONT_SIZE_MAX = 28;
export const FONT_SIZE_DEFAULT = 13;
export interface KeyEventLike { key: string; code: string; shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean; isComposing: boolean }
export interface WheelEventLike { deltaY: number; deltaMode: number; clientX: number; clientY: number }

export function shiftEnterSequence(event: { key: string; shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean; isComposing: boolean }): string | null {
  const plain = event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing;
  return event.key === "Enter" && plain ? "\x1b[13;2u" : null;
}
export function zoomAction(event: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean }): "in" | "out" | "reset" | null {
  if (!event.metaKey || event.ctrlKey || event.altKey) return null;
  if (event.key === "=" || event.key === "+") return "in";
  if (event.key === "-" || event.key === "_") return "out";
  if (event.key === "0") return "reset";
  return null;
}
export function nextFontSize(current: number, action: "in" | "out" | "reset"): number {
  if (action === "reset") return FONT_SIZE_DEFAULT;
  const next = current + (action === "in" ? 1 : -1);
  return Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, next));
}
export function createWheelAccumulator(): { push(deltaY: number, deltaMode: number, cellHeight: number): number } {
  let remainder = 0;
  return {
    push(deltaY, deltaMode, cellHeight) {
      const px = deltaMode === 1 ? deltaY * cellHeight : deltaMode === 2 ? deltaY * 10 * cellHeight : deltaY;
      if (px !== 0 && Math.sign(px) !== Math.sign(remainder)) remainder = 0;
      remainder += px;
      const steps = Math.trunc(remainder / Math.max(24, 1.5 * cellHeight)) + 0;
      remainder -= steps * Math.max(24, 1.5 * cellHeight);
      return steps;
    },
  };
}
export function isCmdV(event: { code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }): boolean {
  return event.code === "KeyV" && event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
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
  const wheel = createWheelAccumulator();
  engine.setKeyHandler((event) => {
    const sequence = shiftEnterSequence(event);
    if (sequence !== null) {
      engine.input(sequence);
      return true;
    }
    const action = zoomAction(event);
    if (action !== null) {
      engine.setFontSize(nextFontSize(engine.fontSize(), action));
      engine.fit();
      return true;
    }
    if (isCmdV(event)) {
      deps.clipboard.readText().then((text) => {
        if (text !== "") engine.paste(text);
      }).catch(deps.onError);
      return true;
    }
    return false;
  });
  engine.setWheelHandler((event) => {
    if (!engine.appOwnsWheel()) return false;
    const steps = wheel.push(event.deltaY, event.deltaMode, engine.cellHeight());
    if (steps !== 0) {
      const step = engine.wheelStep(steps < 0 ? "up" : "down", event.clientX, event.clientY);
      engine.input(step.repeat(Math.abs(steps)));
    }
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
