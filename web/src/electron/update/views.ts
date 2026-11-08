import type { UpdateExec, UpdateFs } from './types';

/**
 * Where a view is built under its checkout. The prod view is the default build of the latest commit
 * on main, in `web/dist-electron` (air-d89 point 4). The preview view is the prod build of the
 * preview branch, kept apart from the prod view so both can be on disk at once (air-d96).
 */
export const PROD_VIEW_DIR = 'dist-electron';
export const PREVIEW_VIEW_DIR = 'dist-electron.preview';
/** The sha the build was made from, the same stamp the prod bundle carries. */
export const BUILT_SHA_FILE = '.built-sha';
/** The one command every view is built with — prod and preview alike (air-d89 point 3, air-d96). */
export const VIEW_BUILD_ARGS: readonly string[] = ['--filter', 'cpd-web-client', 'run', 'build:electron'];

export interface ViewsDeps {
  exec: UpdateExec;
  fs: UpdateFs;
  log?(line: string): void;
}

export type ViewResult = { ok: true; outDir: string; sha: string } | { ok: false; log: string };

/**
 * The latest commit of `checkout` as git reports it. The views are built from a checkout, so the sha
 * is the checkout's HEAD at build time (air-d89 points 3-4).
 */
export async function headSha(exec: UpdateExec, checkout: string): Promise<string> {
  return (await exec('git', ['rev-parse', 'HEAD'], { cwd: checkout })).stdout.trim();
}

/**
 * Build one view from `checkout` into `outDir` with the command prod is built with, and stamp the
 * sha it was built from. The build dir is named to the build through `CPD_ELECTRON_OUT`, the same
 * env the updater's bundle build sets, so prod and preview share one build command (air-d89 point 3).
 */
export async function buildView(
  deps: ViewsDeps,
  opts: { checkout: string; outDir: string; sha: string },
): Promise<ViewResult> {
  deps.log?.(`building view into ${opts.outDir} from ${opts.sha}`);
  const r = await deps.exec('pnpm', [...VIEW_BUILD_ARGS], { cwd: opts.checkout, env: { CPD_ELECTRON_OUT: opts.outDir } });
  if (r.code !== 0) return { ok: false, log: `${r.stderr}\n${r.stdout}` };
  deps.fs.writeText(`${opts.checkout}/${opts.outDir}/${BUILT_SHA_FILE}`, opts.sha);
  return { ok: true, outDir: opts.outDir, sha: opts.sha };
}

/**
 * The prod view: the default build of the latest commit on main, in the checkout's prod build dir
 * (air-d89 point 4).
 */
export async function buildProdView(deps: ViewsDeps, checkout: string): Promise<ViewResult> {
  const sha = await headSha(deps.exec, checkout);
  return buildView(deps, { checkout, outDir: PROD_VIEW_DIR, sha });
}
