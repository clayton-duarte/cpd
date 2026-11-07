import type { UiMode } from "./channels.ts";
import type { HealthRecord } from "./health.ts";
import type { Watchdog } from "./watchdog.ts";

export interface PreviewRunOptions {
  readonly cwd: string;
  readonly env: Readonly<Record<string, string | undefined>>;
}

export interface PreviewRunResult {
  readonly code: number;
  readonly stderr: string;
}

export interface PreviewClientDeps {
  /** The repository root that owns `scripts/preview-web.sh`. */
  readonly root: string;
  /** The preview worktree served by `preview-web.sh`. */
  readonly worktree: string;
  /** The dev renderer URL (`CPD_ELECTRON_DEV_URL`, default `http://localhost:5173/electron.html`). */
  readonly devUrl: string;
  /** The pid of the Electron main process. */
  readonly pid: number;
  /** The current time in epoch milliseconds. */
  readonly now: () => number;
  /** The environment the preview client runs in. */
  readonly env: Readonly<Record<string, string | undefined>>;
  /** The run-only `$CPD_HOME/run/ui-view.json` marker. */
  readonly markerFile: string;
  run(file: string, args: readonly string[], options: PreviewRunOptions): Promise<PreviewRunResult>;
  readHealth(): Promise<HealthRecord | null>;
  writeHealth(record: HealthRecord): Promise<void>;
  showLoading(): void;
  loadDevUrl(url: string): Promise<void>;
  showErrorBox(title: string, content: string): void;
  setMode(mode: UiMode): Promise<void>;
  previousMode(): UiMode;
  removeMarker(): Promise<void>;
  watchdog: Watchdog;
}

export interface PreviewClient {
  /** Runs `preview-web.sh serve` once, writes the loading health and shows the loading page. */
  open(): Promise<void>;
  /** Switching away from the preview: the server stays up. */
  leave(): Promise<void>;
  /** Ticks the health watchdog and loads the dev URL once the health is healthy. */
  tick(): Promise<void>;
  /** Graceful quit: runs `preview-web.sh unserve` once and removes the marker. */
  close(): Promise<void>;
}

export function createPreviewClient(_deps: PreviewClientDeps): PreviewClient {
  throw new Error("preview client is not implemented");
}
