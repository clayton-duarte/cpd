export const CHANNELS = {
  ptyStart: "pty:start",
  ptyWrite: "pty:write",
  ptyResize: "pty:resize",
  ptyData: "pty:data",
  ptyExit: "pty:exit",
  uiGetMode: "ui:get-mode",
  uiMode: "ui:mode",
  wasm: "ghostty:wasm",
} as const;

export type UiMode = "tui" | "webui";

export interface PtySize {
  cols: number;
  rows: number;
}

export interface CpdElectronBridge {
  readWasm(): Promise<Uint8Array>;
  getMode(): Promise<UiMode>;
  startPty(size: PtySize): Promise<void>;
  writePty(data: string): void;
  resizePty(size: PtySize): void;
  onPtyData(cb: (data: string) => void): () => void;
  onPtyExit(cb: (exitCode: number) => void): () => void;
  onMode(cb: (mode: UiMode) => void): () => void;
}
