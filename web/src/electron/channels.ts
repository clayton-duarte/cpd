import type { BootstrapStatus, UpdateState } from "./update/types.ts";
import type { SshHost } from "./ssh-config.ts";

export const CHANNELS = {
  ptyStart: "pty:start",
  ptyWrite: "pty:write",
  ptyResize: "pty:resize",
  ptyData: "pty:data",
  ptyExit: "pty:exit",
  ptyStatus: "pty:status",
  ptyHosts: "pty:hosts",
  uiGetMode: "ui:get-mode",
  uiMode: "ui:mode",
  wasm: "ghostty:wasm",
  webC2s: "cpd:c2s",
  webS2c: "cpd:s2c",
  healthOk: "health:ok",
  healthCrash: "health:crash",
} as const;

export const UPDATE_BOOTSTRAP_CHANNEL = "update:bootstrap";
export const UPDATE_STATE_CHANNEL = "update:state";

export type UiMode = "tui" | "webui";

export interface PtySize {
  cols: number;
  rows: number;
}

/** The lifecycle status of a tab's pty (ELECTRON.md, `## Remote TUI tabs over ssh`). */
export type TabStatus = "connecting" | "live" | "exited" | "failed";

/** The pty payloads of a tab, keyed by `tabId` (`local` or `ssh:<alias>`). */
export interface PtyDataPayload {
  tabId: string;
  data: string;
}

export interface PtyExitPayload {
  tabId: string;
  code: number;
}

export interface PtyStatusPayload {
  tabId: string;
  status: TabStatus;
  /** Present on an `exited` or `failed` status. */
  code?: number;
}

// The `*Pty` methods drive the local, single-pty renderer (terminal.ts) on the un-keyed payloads;
// the `*Tab*` methods drive a per-tab pty on the payloads above. The per-tab methods are optional so
// the existing local bridge mock (terminal.test.ts) still satisfies this interface; the renderer does
// not call them yet.
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
  /** Starts (or restarts) that tab's pty. */
  startTabPty?(tabId: string, size: PtySize): Promise<void>;
  writeTabPty?(tabId: string, data: string): void;
  resizeTabPty?(tabId: string, size: PtySize): void;
  onTabData?(cb: (payload: PtyDataPayload) => void): () => void;
  onTabExit?(cb: (payload: PtyExitPayload) => void): () => void;
  onTabStatus?(cb: (payload: PtyStatusPayload) => void): () => void;
  /** The resolved ssh hosts from `~/.ssh/config`, in tab order. */
  listHosts?(): Promise<SshHost[]>;
}

export interface UpdateBridge {
  onUpdateBootstrap(cb: (s: BootstrapStatus) => void): () => void;
  onUpdateState(cb: (s: UpdateState) => void): () => void;
}
