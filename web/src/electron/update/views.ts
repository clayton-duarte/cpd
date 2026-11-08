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
 * Build one view from `checkout` into `outDir` with the command prod is built with, and stamp the
 * sha it was built from. Out of scope for this Job: the implementing change fills this in.
 */
export function buildView(
  deps: ViewsDeps,
  opts: { checkout: string; outDir: string; sha: string },
): Promise<ViewResult> {
  throw new Error('buildView is not implemented');
}

/**
 * The prod view: the default build of the latest commit on main, in the checkout's prod build dir
 * (air-d89 point 4). Out of scope for this Job: the implementing change fills this in.
 */
export function buildProdView(deps: ViewsDeps, checkout: string): Promise<ViewResult> {
  throw new Error('buildProdView is not implemented');
}
