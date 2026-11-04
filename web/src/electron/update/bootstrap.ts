import type { BootstrapStatus, UpdateExec, UpdateFs } from './types';

export async function ensureCli(deps: { platform: string; home: string; root: string | null; exec: UpdateExec; fs: UpdateFs; onStatus: (s: BootstrapStatus) => void }): Promise<BootstrapStatus> {
  const bin = `${deps.home}/.local/bin/cpd`;
  const logPath = `${deps.home}/.cpd/run/cli-bootstrap.log`;
  const done = (s: BootstrapStatus): BootstrapStatus => (deps.onStatus(s), s);
  if (deps.platform !== 'darwin' || deps.fs.exists(bin)) return done({ state: 'ready' });
  if (deps.root === null) return done({ state: 'failed', error: 'cpd checkout not found: set CPD_CHECKOUT', logPath });
  deps.onStatus({ state: 'installing' });
  const r = await deps.exec('scripts/install-cpd.sh', [], { cwd: deps.root });
  if (r.code !== 0) {
    deps.fs.writeText(logPath, `${r.stdout}\n${r.stderr}`);
    const last = r.stderr.split('\n').map((l) => l.trim()).filter((l) => l !== '').pop() ?? `install-cpd.sh exited ${r.code}`;
    return done({ state: 'failed', error: last, logPath });
  }
  if (!deps.fs.exists(bin)) return done({ state: 'failed', error: `${bin} missing after install`, logPath });
  return done({ state: 'ready' });
}
