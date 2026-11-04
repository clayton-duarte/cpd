import type { BootstrapStatus, UpdateState } from "./update/types.ts";

export const CHANNELS = {
  ptyStart: "pty:start",
  ptyWrite: "pty:write",
  ptyResize: "pty:resize",
  ptyData: "pty:data",
  ptyExit: "pty:exit",
  uiGetMode: "ui:get-mode",
  uiMode: "ui:mode",
  wasm: "ghostty:wasm",
  webC2s: "cpd:c2s",
  webS2c: "cpd:s2c",
} as const;

export const UPDATE_BOOTSTRAP_CHANNEL = "update:bootstrap";
export const UPDATE_STATE_CHANNEL = "update:state";

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
  sendWeb(raw: unknown): void;
  onWebMessage(cb: (raw: unknown) => void): () => void;
}

export interface UpdateBridge {
  onUpdateBootstrap(cb: (s: BootstrapStatus) => void): () => void;
  onUpdateState(cb: (s: UpdateState) => void): () => void;
}
