import { BUILT_SHA_FILE, swapBundle } from './bundle';
import type { SyncResult } from './checkout';
import type { UpdateFs, UpdateState } from './types';

export interface UpdaterDeps { sync(): Promise<SyncResult>; headSha(): Promise<string>; builtSha(): string | null; cliSha(): string | null; build(opts: { lockfileChanged: boolean; sha: string }): Promise<{ ok: true } | { ok: false; log: string }>; swap(): { mainChanged: boolean }; whenApplicable(): Promise<void>; reloadRenderer(): void; relaunch?(): void; installCli(sha: string): Promise<{ ok: true } | { ok: false; log: string }>; rebuildViews(): Promise<void>; onState(s: UpdateState): void; log(line: string): void }

export interface PromoteDeps { fs: UpdateFs; root: string; headSha(): Promise<string> }

/**
 * Launch promote (web/CONTRACT.md §19.1): before the window opens, `web/dist-electron.next` is swapped
 * into `web/dist-electron` when its `.built-sha` equals `git rev-parse HEAD`; otherwise nothing happens.
 */
export async function promoteOnLaunch(deps: PromoteDeps): Promise<void> {
  const nextSha = deps.fs.readText(`${deps.root}/web/dist-electron.next/${BUILT_SHA_FILE}`)?.trim();
  if (!nextSha) return;
  const head = (await deps.headSha()).trim();
  if (nextSha !== head) return;
  swapBundle(deps.fs, deps.root);
}

export function createUpdater(deps: UpdaterDeps): { check(): Promise<void> } {
  let restartNeeded = false;
  let current: Promise<void> | null = null;
  let queued = false;

  async function bundleFlow(sha: string, lockfileChanged: boolean): Promise<void> {
    if (deps.builtSha() === sha) {
      deps.onState({ phase: 'idle' });
      return;
    }
    deps.onState({ phase: 'building' });
    const built = await deps.build({ lockfileChanged, sha });
    if (!built.ok) {
      deps.log(built.log);
      const lines = built.log.split('\n').map((l) => l.trim()).filter((l) => l !== '');
      deps.onState({ phase: 'error', message: lines[lines.length - 1] ?? 'build failed' });
      return;
    }
    deps.onState({ phase: 'waiting' });
    await deps.whenApplicable();
    const { mainChanged } = deps.swap();
    if (mainChanged || restartNeeded) {
      restartNeeded = true;
      deps.onState({ phase: 'restart-needed' });
      deps.relaunch?.();
      return;
    }
    deps.reloadRenderer();
    deps.onState({ phase: 'idle' });
  }

  async function cliFlow(sha: string): Promise<void> {
    if (deps.cliSha() === sha) return;
    const r = await deps.installCli(sha);
    if (!r.ok) {
      deps.log(`cli install failed: ${r.log}`);
      return;
    }
    // The CLI update landed: rebuild the views from the checkout so the new build becomes the prod
    // view and the preview branch is rebuilt alongside it (air-d89 point 6).
    await deps.rebuildViews();
  }

  async function run(): Promise<void> {
    deps.onState({ phase: 'syncing' });
    const synced = await deps.sync();
    if (synced.kind === 'skipped') {
      deps.log(`update skipped: ${synced.reason}`);
      deps.onState({ phase: 'idle' });
      return;
    }
    const sha = await deps.headSha();
    const lockfileChanged = synced.kind === 'updated' ? synced.lockfileChanged : true;
    await Promise.all([bundleFlow(sha, lockfileChanged), cliFlow(sha)]);
  }

  async function loop(): Promise<void> {
    try {
      await run();
      while (queued) {
        queued = false;
        await run();
      }
    } finally {
      current = null;
    }
  }

  return {
    check() {
      if (current) {
        queued = true;
        return current;
      }
      current = loop();
      return current;
    },
  };
}
