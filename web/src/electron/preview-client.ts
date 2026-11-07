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

export function createPreviewClient(deps: PreviewClientDeps): PreviewClient {
  const script = `${deps.root}/scripts/preview-web.sh`;
  let active = false;
  let served = false;
  let closed = false;
  let loaded = false;
  let reloadEpoch = 0;

  const run = (args: readonly string[]): Promise<PreviewRunResult> =>
    deps.run("sh", [script, ...args], { cwd: deps.root, env: deps.env });

  const fail = async (message: string): Promise<void> => {
    deps.showErrorBox("WebUI Preview", message);
    await deps.setMode(deps.previousMode());
  };

  const message = (error: unknown, fallback: string): string => {
    const text = error instanceof Error ? error.message : String(error);
    return text.trim() || fallback;
  };

  return {
    async open(): Promise<void> {
      if (active || closed) return;
      active = true;
      if (!served) {
        let result: PreviewRunResult;
        try {
          result = await run(["serve", deps.worktree]);
        } catch (error) {
          active = false;
          await fail(message(error, "preview server failed to start"));
          return;
        }
        if (result.code !== 0) {
          active = false;
          await fail(result.stderr.trim() || `preview server exited ${result.code}`);
          return;
        }
        served = true;
      }
      reloadEpoch += 1;
      await deps.writeHealth({
        status: "loading",
        reloadEpoch,
        pid: deps.pid,
        startedAt: new Date(deps.now()).toISOString(),
      });
      deps.showLoading();
    },

    async leave(): Promise<void> {
      // The server stays up; a later open() re-shows the loading page and reloads the dev URL.
      active = false;
      loaded = false;
    },

    async tick(): Promise<void> {
      if (!active) return;
      await deps.watchdog.tick();
      if (loaded) return;
      const record = await deps.readHealth();
      if (record === null || record.status !== "healthy") return;
      loaded = true;
      try {
        await deps.loadDevUrl(deps.devUrl);
      } catch (error) {
        loaded = false;
        active = false;
        await fail(message(error, `dev URL ${deps.devUrl} failed to load`));
      }
    },

    async close(): Promise<void> {
      if (closed) return;
      closed = true;
      active = false;
      served = false;
      await run(["unserve"]);
      await deps.removeMarker();
    },
  };
}
