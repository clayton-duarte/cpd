import { toArgv, type CpdCommand } from '../../server/cpd-argv.ts';
import type { UpdateExec, UpdateExecResult, UpdateFs, UpdateTimers } from './types';

const stampPath = (root: string) => `${root}/target/.cli-installed-sha`;
const logOf = (r: { stderr: string; stdout: string }) => `${r.stderr}${r.stdout}`;

/** The one JSON error envelope cpd prints on stdout (commands.md §1.2); a refusal carries `error.code` and `error.message`. */
interface Envelope {
  error?: { code?: unknown; message?: unknown };
}

/**
 * The message of a failed cpd call. cpd prints every error, a refusal included, as one JSON envelope on stdout and
 * leaves stderr empty, so when stderr is empty the envelope's `error.code` and `error.message` name the failure.
 */
function failureOf(name: string, r: UpdateExecResult): string {
  const head = `${name} failed (exit ${r.code})`;
  if (r.stderr.trim() !== '') return `${head}: ${r.stderr}`;
  let envelope: Envelope | null = null;
  try {
    envelope = JSON.parse(r.stdout) as Envelope;
  } catch {
    envelope = null;
  }
  const code = envelope?.error?.code;
  const message = envelope?.error?.message;
  if (typeof code === 'string' && typeof message === 'string') return `${head}: ${code}: ${message}`;
  return `${head}: ${r.stdout}`;
}

async function cpdJson(exec: UpdateExec, root: string, cmd: CpdCommand): Promise<unknown> {
  const args = toArgv(cmd);
  const name = `cpd ${args.join(' ')}`;
  const r = await exec('cpd', args, { cwd: root });
  if (r.code !== 0) throw new Error(failureOf(name, r));
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

/**
 * Writes the binary at once: `--no-follow-up` skips the install's disruptive follow-ups, and the binary write inside
 * the install is atomic (renamed over the destination), so a running `cpd` keeps its old inode. The full install that
 * follows carries the follow-ups, the engine restart included; restarting with running attempts is safe.
 */
async function installBinary(deps: { exec: UpdateExec; log: (line: string) => void }, root: string): Promise<void> {
  try {
    const r = await deps.exec('scripts/install-cpd.sh', ['--no-follow-up'], { cwd: root });
    if (r.code !== 0) deps.log(`cli install (binary only) failed: ${logOf(r)}`);
  } catch (e) {
    deps.log(`cli install (binary only) failed: ${(e as Error).message}`);
  }
}

export async function installCli(deps: { exec: UpdateExec; fs: UpdateFs; timers: UpdateTimers; log: (line: string) => void }, root: string, sha: string): Promise<{ ok: true } | { ok: false; log: string }> {
  const build = await deps.exec('scripts/cargo.sh', ['build', '--release', '--locked', '-p', 'cpd'], { cwd: root });
  if (build.code !== 0) return { ok: false, log: logOf(build) };
  await installBinary(deps, root);
  const install = await deps.exec('scripts/install-cpd.sh', [], { cwd: root });
  if (install.code !== 0) return { ok: false, log: logOf(install) };
  deps.fs.writeText(stampPath(root), sha);
  return { ok: true };
}

export function readInstalledSha(fs: UpdateFs, root: string): string | null {
  const t = fs.readText(stampPath(root));
  return t === null ? null : t.trim();
}
