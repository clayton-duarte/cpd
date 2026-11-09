import path from 'node:path';
import { ensureCli } from './bootstrap';
import { buildBundle, readBuiltSha, swapBundle } from './bundle';
import { resolveCheckout, syncCheckout } from './checkout';
import { installCli, readInstalledSha } from './cli';
import { createUpdater } from './controller';
import { rebuildViews } from './preview';
import { followMerges } from './follow';
import type { SpawnFollow } from './follow';
import { readPreviewMode } from '../preview-mode';
import { whenApplicable, type ViewChanges } from './idle';
import type { BootstrapStatus, UpdateExec, UpdateFs, UpdateState, UpdateTimers } from './types';

export interface AutoUpdateDeps { env: Record<string, string | undefined>; bundleDir: string; home: string; platform: string; exec: UpdateExec; fs: UpdateFs; timers: UpdateTimers; spawnFollow: SpawnFollow; isFocused: () => boolean; systemIdleSeconds: () => number; viewChanges?: ViewChanges; reloadRenderer: () => void; onState: (s: UpdateState) => void; onBootstrap: (s: BootstrapStatus) => void; log: (line: string) => void }

export async function startAutoUpdate(deps: AutoUpdateDeps): Promise<{ bootstrap: BootstrapStatus; stop(): void }> {
  const root = resolveCheckout({ env: deps.env, bundleDir: deps.bundleDir, fs: deps.fs });
  const bootstrap = await ensureCli({ platform: deps.platform, home: deps.home, root, exec: deps.exec, fs: deps.fs, onStatus: deps.onBootstrap });
  if (bootstrap.state !== 'ready') return { bootstrap, stop: () => {} };
  if (root === null) {
    deps.log('auto-update off: cpd checkout not found');
    return { bootstrap, stop: () => {} };
  }
  const rebase = readPreviewMode(deps.env).mode === 'preview';
  const updater = createUpdater({
    sync: () => syncCheckout(deps.exec, root, { rebase }),
    headSha: async () => (await deps.exec('git', ['rev-parse', 'HEAD'], { cwd: root })).stdout.trim(),
    builtSha: () => readBuiltSha(deps.fs, root),
    cliSha: () => readInstalledSha(deps.fs, root),
    build: (opts) => buildBundle({ exec: deps.exec, fs: deps.fs }, root, opts),
    swap: () => swapBundle(deps.fs, root),
    whenApplicable: () => whenApplicable({ isFocused: deps.isFocused, systemIdleSeconds: deps.systemIdleSeconds, timers: deps.timers, viewChanges: deps.viewChanges }),
    reloadRenderer: deps.reloadRenderer,
    installCli: (sha) => installCli({ exec: deps.exec, fs: deps.fs, timers: deps.timers, log: deps.log }, root, sha),
    rebuildViews: async () => {
      const previewRoot = deps.env['CPD_WEB_PREVIEW_WORKTREE'] ?? path.resolve(root, '..', 'cpd.preview.web');
      const r = await rebuildViews({ exec: deps.exec, fs: deps.fs, log: deps.log }, { prod: root, preview: previewRoot });
      if (!r.prod.ok) deps.log(`prod view build failed: ${r.prod.log}`);
      if (!r.preview.ok) deps.log(`preview view build failed: ${r.preview.log}`);
    },
    onState: deps.onState,
    log: deps.log,
  });
  const logError = (e: unknown): void => deps.log(`update check failed: ${e instanceof Error ? e.message : String(e)}`);
  const follow = followMerges({
    spawn: deps.spawnFollow,
    timers: deps.timers,
    onMerged: () => void updater.check().catch(logError),
    onError: logError,
  });
  return { bootstrap, stop: () => follow.stop() };
}
