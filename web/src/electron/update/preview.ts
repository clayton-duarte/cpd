import type { UpdateExec } from './types';
import { buildView, PREVIEW_VIEW_DIR, type ViewResult, type ViewsDeps } from './views';

/**
 * The preview branch: a long-lived branch off main, a mirror of main carrying changes on top
 * before they are committed (air-d96). The preview view is the prod build of this branch.
 */
export const PREVIEW_BRANCH = 'preview';

/**
 * The branch the preview checkout is on. Out of scope for this Job: the implementing change fills
 * this in.
 */
export function previewBranch(exec: UpdateExec, checkout: string): Promise<string> {
  throw new Error('previewBranch is not implemented');
}

/**
 * The preview view: build the preview branch the way prod is built, into its own build dir, and
 * stamp the sha (air-d96). Refuses a checkout that is not on the preview branch.
 * Out of scope for this Job: the implementing change fills this in.
 */
export function buildPreviewView(deps: ViewsDeps, checkout: string): Promise<ViewResult> {
  throw new Error('buildPreviewView is not implemented');
}

/**
 * Rebuild both views from their checkouts when the CLI update lands (air-d89 point 6): the prod view
 * from main first, then the preview view from the preview branch.
 * Out of scope for this Job: the implementing change fills this in.
 */
export function rebuildViews(
  deps: ViewsDeps,
  opts: { prod: string; preview: string },
): Promise<{ prod: ViewResult; preview: ViewResult }> {
  throw new Error('rebuildViews is not implemented');
}
