import type { UpdateExec } from './types';
import { buildProdView, buildView, headSha, PREVIEW_VIEW_DIR, type ViewResult, type ViewsDeps } from './views';

/**
 * The preview branch: a long-lived branch off main, a mirror of main carrying changes on top
 * before they are committed (air-d96). The preview view is the prod build of this branch.
 */
export const PREVIEW_BRANCH = 'preview';

/**
 * The branch the preview checkout is on (air-d96). The preview view is the prod build of the
 * preview branch, so the checkout must be on it before it is built.
 */
export async function previewBranch(exec: UpdateExec, checkout: string): Promise<string> {
  return (await exec('git', ['rev-parse', '--abbrev-ref', 'HEAD'], { cwd: checkout })).stdout.trim();
}

/**
 * The preview view: build the preview branch the way prod is built, into its own build dir, and
 * stamp the sha (air-d96). Refuses a checkout that is not on the preview branch.
 */
export async function buildPreviewView(deps: ViewsDeps, checkout: string): Promise<ViewResult> {
  const branch = await previewBranch(deps.exec, checkout);
  if (branch !== PREVIEW_BRANCH) {
    return { ok: false, log: `preview checkout ${checkout} is on ${branch}, not ${PREVIEW_BRANCH}` };
  }
  const sha = await headSha(deps.exec, checkout);
  return buildView(deps, { checkout, outDir: PREVIEW_VIEW_DIR, sha });
}

/**
 * Rebuild both views from their checkouts when the CLI update lands (air-d89 point 6): the prod view
 * from main first, then the preview view from the preview branch, so a new build becomes the view.
 */
export async function rebuildViews(
  deps: ViewsDeps,
  opts: { prod: string; preview: string },
): Promise<{ prod: ViewResult; preview: ViewResult }> {
  const prod = await buildProdView(deps, opts.prod);
  const preview = await buildPreviewView(deps, opts.preview);
  return { prod, preview };
}
