/** The dev renderer URL the in-place preview loads when `CPD_ELECTRON_DEV_URL` is unset. */
export const DEFAULT_DEV_URL = 'http://localhost:5173/electron.html';

/**
 * The explicit mode request a relaunch carries as an argument. Electron's `app.relaunch()`
 * boots the new process with the original environment, so a mutated `process.env` never
 * reaches it; the argument does, and beats `CPD_MODE` (air-d95).
 */
export const MODE_ARG = '--cpd-mode=';

export type PreviewMode = { readonly mode: 'production' } | { readonly mode: 'preview'; readonly checkout: string; readonly devUrl: string };

export function readPreviewMode(
  env: Readonly<Record<string, string | undefined>>,
  args: readonly string[] = [],
): PreviewMode {
  const requested = args.find((arg) => arg.startsWith(MODE_ARG))?.slice(MODE_ARG.length);
  const mode = requested ?? (env['CPD_MODE'] || 'production');
  if (mode === 'production') return { mode };
  if (mode !== 'preview') throw new Error(`CPD_MODE must be production or preview, got "${mode}"`);
  const checkout = env['CPD_CHECKOUT'];
  if (!checkout) throw new Error('CPD_MODE=preview needs CPD_CHECKOUT');
  return { mode, checkout, devUrl: env['CPD_ELECTRON_DEV_URL'] || DEFAULT_DEV_URL };
}
