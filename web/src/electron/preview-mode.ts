export type PreviewMode = { readonly mode: 'production' } | { readonly mode: 'preview'; readonly checkout: string; readonly devUrl: string | null };

export function readPreviewMode(env: Readonly<Record<string, string | undefined>>): PreviewMode {
  const mode = env['CPD_MODE'] || 'production';
  if (mode === 'production') return { mode };
  if (mode !== 'preview') throw new Error(`CPD_MODE must be production or preview, got "${mode}"`);
  const checkout = env['CPD_CHECKOUT'];
  if (!checkout) throw new Error('CPD_MODE=preview needs CPD_CHECKOUT');
  return { mode, checkout, devUrl: env['CPD_ELECTRON_DEV_URL'] || null };
}
