export type PreviewMode = { readonly mode: 'production' } | { readonly mode: 'preview'; readonly checkout: string; readonly devUrl: string | null };

export function readPreviewMode(_env: Readonly<Record<string, string | undefined>>): PreviewMode {
  throw new Error('not implemented');
}
