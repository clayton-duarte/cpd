import type { UpdateExec, UpdateFs } from './types';

export const BUILT_SHA_FILE = '.built-sha';
const cur = (root: string) => `${root}/web/dist-electron`;
const next = (root: string) => `${root}/web/dist-electron.next`;

export async function buildBundle(deps: { exec: UpdateExec; fs: UpdateFs }, root: string, opts: { lockfileChanged: boolean; sha: string }): Promise<{ ok: true } | { ok: false; log: string }> {
  deps.fs.remove(next(root));
  const fail = (r: { stdout: string; stderr: string }) => ({ ok: false as const, log: `${r.stderr}\n${r.stdout}` });
  if (opts.lockfileChanged) {
    const r = await deps.exec('pnpm', ['install', '--frozen-lockfile'], { cwd: root });
    if (r.code !== 0) return fail(r);
  }
  const b = await deps.exec('pnpm', ['--filter', 'cpd-web-client', 'run', 'build:electron'], { cwd: root, env: { CPD_ELECTRON_OUT: 'dist-electron.next' } });
  if (b.code !== 0) return fail(b);
  deps.fs.writeText(`${next(root)}/${BUILT_SHA_FILE}`, opts.sha);
  return { ok: true };
}
export function readBuiltSha(fs: UpdateFs, root: string): string | null {
  return fs.readText(`${cur(root)}/${BUILT_SHA_FILE}`)?.trim() ?? null;
}
export function swapBundle(fs: UpdateFs, root: string): { mainChanged: boolean } {
  const differs = (f: string) => {
    const a = fs.readText(`${cur(root)}/${f}`);
    return a === null || a !== fs.readText(`${next(root)}/${f}`);
  };
  const mainChanged = differs('main.js') || differs('preload.cjs');
  const prev = `${cur(root)}.prev`;
  fs.rename(cur(root), prev);
  try {
    fs.rename(next(root), cur(root));
  } catch (e) {
    fs.rename(prev, cur(root));
    throw e;
  }
  fs.remove(prev);
  return { mainChanged };
}
