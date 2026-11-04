export function electronOutDir(env: Record<string, string | undefined>): string {
  const v = env['CPD_ELECTRON_OUT'];
  if (v === undefined) return 'dist-electron';
  if (!/^[A-Za-z0-9._-]+$/.test(v) || v === '.' || v === '..') throw new Error(`CPD_ELECTRON_OUT must be a single relative segment: ${JSON.stringify(v)}`);
  return v;
}
