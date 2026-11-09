import type { UpdateExec, UpdateFs } from './types';

export const BUILT_SHA_FILE = '.built-sha';
// D1: the main-side build artefacts Electron loads from dist-electron but does not pick up on a
// renderer reload — a change to any of them needs the whole process relaunched. The renderer output
// lives under dist-electron/renderer and is the only thing an in-place reload refreshes.
export const MAIN_ARTEFACTS = ['main.js', 'preload.cjs', 'server.js'] as const;
const cur = (root: string) => `${root}/web/dist-electron`;
const next = (root: string) => `${root}/web/dist-electron.next`;

/**
 * Where the previous prod builds are kept on disk after a swap, newest first: HEAD-1 then HEAD-2
 * (air-d95). The current build stays at `web/dist-electron`; these are the slots of the revert
 * ladder below prod (air-d88), so a bad build always has a previous build to roll back to.
 */
export const previousBuildDirs = (root: string): readonly [string, string] => [`${cur(root)}.head-1`, `${cur(root)}.head-2`];

export async function buildBundle(deps: { exec: UpdateExec; fs: UpdateFs }, root: string, opts: { lockfileChanged: boolean; sha: string }): Promise<{ ok: true } | { ok: false; log: string }> {
  deps.fs.remove(next(root));
  const fail = (r: { stdout: string; stderr: string }) => ({ ok: false as const, log: `${r.stderr}\n${r.stdout}` });
  if (opts.lockfileChanged) {
    const r = await deps.exec('pnpm', ['install', '--frozen-lockfile'], { cwd: root });
    if (r.code !== 0) return fail(r);
  }
  const b = await deps.exec('pnpm', ['--filter', 'cpd-web-client', 'run', 'build:electron'], { cwd: root, env: { CPD_ELECTRON_OUT: 'dist-electron.next', CPD_WEB_SHA: opts.sha } });
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
    const b = fs.readText(`${next(root)}/${f}`);
    // A file absent from both builds is not a change; one present on either side that differs is.
    return (a ?? b) !== null && a !== b;
  };
  const mainChanged = MAIN_ARTEFACTS.some(differs);
  const [head1, head2] = previousBuildDirs(root);
  const prev = `${cur(root)}.prev`;
  fs.rename(cur(root), prev);
  try {
    fs.rename(next(root), cur(root));
  } catch (e) {
    fs.rename(prev, cur(root));
    throw e;
  }
  // Rotate the revert ladder: the displaced prod build becomes HEAD-1, demoting the former HEAD-1 to HEAD-2 and
  // pruning anything older, so the current build, HEAD-1 and HEAD-2 stay on disk (air-d95).
  fs.remove(head2);
  fs.rename(head1, head2);
  fs.rename(prev, head1);
  return { mainChanged };
}
