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

async function cpdJson(exec: UpdateExec, root: string, args: string[]): Promise<unknown> {
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
  const plans = (await cpdJson(exec, root, ['plan', 'list'])) as { plans: { id: string }[] };
  let count = 0;
  for (const { id } of plans.plans) {
    const list = (await cpdJson(exec, root, ['attempt', 'list', '--active', '--plan', id])) as { attempts: { status: string }[] };
    count += list.attempts.filter((a) => a.status === 'running').length;
  }
  return count;
}

/**
 * Writes the binary at once, without waiting for running attempts: `--no-follow-up` skips the install's disruptive
 * follow-ups, and the binary write inside the install is atomic (renamed over the destination), so a running `cpd`
 * keeps its old inode. Non-fatal: the install at zero running attempts runs the whole install again, follow-ups
 * included, so a failure here is logged and retried there.
 */
async function installBinary(deps: { exec: UpdateExec; log: (line: string) => void }, root: string): Promise<void> {
  try {
    const r = await deps.exec('scripts/install-cpd.sh', ['--no-follow-up'], { cwd: root });
    if (r.code !== 0) deps.log(`cli install (binary only) failed: ${logOf(r)}`);
  } catch (e) {
    deps.log(`cli install (binary only) failed: ${(e as Error).message}`);
  }
}

export async function installCli(deps: { exec: UpdateExec; fs: UpdateFs; timers: UpdateTimers; log: (line: string) => void; retryMs?: number }, root: string, sha: string): Promise<{ ok: true } | { ok: false; log: string }> {
  const build = await deps.exec('scripts/cargo.sh', ['build', '--release', '--locked', '-p', 'cpd'], { cwd: root });
  if (build.code !== 0) return { ok: false, log: logOf(build) };
  const retryMs = deps.retryMs ?? 60000;
  await installBinary(deps, root);
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
