import { toArgv, type CpdCommand } from '../../server/cpd-argv.ts';
import type { UpdateExec, UpdateFs, UpdateTimers } from './types';

const stampPath = (root: string) => `${root}/target/.cli-installed-sha`;
const logOf = (r: { stderr: string; stdout: string }) => `${r.stderr}${r.stdout}`;

async function cpdJson(exec: UpdateExec, root: string, cmd: CpdCommand): Promise<unknown> {
  const args = toArgv(cmd);
  const name = `cpd ${args.join(' ')}`;
  const r = await exec('cpd', args, { cwd: root });
  if (r.code !== 0) throw new Error(`${name} failed (exit ${r.code}): ${r.stderr}`);
  try {
    return JSON.parse(r.stdout);
  } catch {
    throw new Error(`${name} printed invalid JSON`);
  }
}

export async function countRunningAttempts(exec: UpdateExec, root: string): Promise<number> {
  const plans = (await cpdJson(exec, root, { kind: 'plan.list' })) as { plans: { id: string }[] };
  let count = 0;
  for (const { id } of plans.plans) {
    const list = (await cpdJson(exec, root, { kind: 'attempt.list', planId: id, active: true })) as { attempts: { status: string }[] };
    count += list.attempts.filter((a) => a.status === 'running').length;
  }
  return count;
}

export async function installCli(deps: { exec: UpdateExec; fs: UpdateFs; timers: UpdateTimers; log: (line: string) => void; retryMs?: number }, root: string, sha: string): Promise<{ ok: true } | { ok: false; log: string }> {
  const build = await deps.exec('scripts/cargo.sh', ['build', '--release', '--locked', '-p', 'cpd'], { cwd: root });
  if (build.code !== 0) return { ok: false, log: logOf(build) };
  const retryMs = deps.retryMs ?? 60000;
  for (;;) {
    let count: number;
    try {
      count = await countRunningAttempts(deps.exec, root);
    } catch (e) {
      return { ok: false, log: (e as Error).message };
    }
    if (count === 0) break;
    deps.log(`cli install waiting: ${count} attempt(s) running`);
    await new Promise<void>((resolve) => deps.timers.setTimeout(resolve, retryMs));
  }
  const install = await deps.exec('scripts/install-cpd.sh', [], { cwd: root });
  if (install.code !== 0) return { ok: false, log: logOf(install) };
  deps.fs.writeText(stampPath(root), sha);
  return { ok: true };
}

export function readInstalledSha(fs: UpdateFs, root: string): string | null {
  const t = fs.readText(stampPath(root));
  return t === null ? null : t.trim();
}
