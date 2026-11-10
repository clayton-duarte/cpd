/**
 * Port-in-use holder detection.
 * Identifies the process and PID holding a TCP port using lsof (macOS/POSIX) or ss (Linux).
 * Decided by pro-d542, web/CONTRACT.md §15.1.
 */

import { execFile } from 'node:child_process';

export interface PortHolder {
  process: string;
  pid: number;
}

export interface PortHolderOptions {
  execCommand?: (command: string, args: string[]) => Promise<{ stdout: string; stderr: string; exitCode: number }>;
}

async function defaultExecCommand(command: string, args: string[]): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  return new Promise((resolve) => {
    execFile(command, args, { timeout: 10_000 }, (error, stdout, stderr) => {
      const exitCode = error ? (typeof (error as { code?: unknown }).code === 'number' ? (error as { code: number }).code : 1) : 0;
      resolve({
        stdout: typeof stdout === 'string' ? stdout : '',
        stderr: typeof stderr === 'string' ? stderr : '',
        exitCode,
      });
    });
  });
}

/**
 * Parses `lsof -nP -iTCP:<port> -sTCP:LISTEN -Fpcn` output.
 * Lines start with single character field tags:
 *   p<pid>
 *   c<command>
 *   n<name>
 */
function parseLsofOutput(output: string): PortHolder | null {
  let pid: number | null = null;
  let processName: string | null = null;

  for (const line of output.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const tag = trimmed[0];
    const value = trimmed.slice(1);
    if (tag === 'p') {
      const parsed = parseInt(value, 10);
      if (!Number.isNaN(parsed)) {
        pid = parsed;
      }
    } else if (tag === 'c') {
      processName = value;
    }
    if (pid !== null && processName !== null) {
      return { pid, process: processName };
    }
  }
  if (pid !== null && processName !== null) {
    return { pid, process: processName };
  }
  return null;
}

/**
 * Parses `ss` output.
 * Example format:
 *   LISTEN 0 128 0.0.0.0:33333 0.0.0.0:* users:(("vite",pid=54321,fd=4))
 */
function parseSsOutput(output: string): PortHolder | null {
  const match = output.match(/users:\(\("([^"]+)",pid=(\d+)/);
  if (match && match[1] && match[2]) {
    const pid = parseInt(match[2], 10);
    if (!Number.isNaN(pid)) {
      return { process: match[1], pid };
    }
  }
  return null;
}

export async function detectPortHolder(port: number, options?: PortHolderOptions): Promise<PortHolder | null> {
  const exec = options?.execCommand ?? defaultExecCommand;

  // 1. Try lsof
  try {
    const lsofRes = await exec('lsof', ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN', '-Fpcn']);
    if (lsofRes.exitCode === 0 && lsofRes.stdout) {
      const holder = parseLsofOutput(lsofRes.stdout);
      if (holder) return holder;
    }
  } catch {
    // ignore and try fallback
  }

  // 2. Try ss fallback
  try {
    const ssRes = await exec('ss', ['-H', '-ltnp', `sport = :${port}`]);
    if (ssRes.exitCode === 0 && ssRes.stdout) {
      const holder = parseSsOutput(ssRes.stdout);
      if (holder) return holder;
    }
  } catch {
    // ignore
  }

  return null;
}

export async function formatPortInUseMessage(port: number, options?: PortHolderOptions): Promise<string> {
  const holder = await detectPortHolder(port, options);
  if (holder) {
    return `Port ${port} is in use by ${holder.process} (PID ${holder.pid}). Exiting.`;
  }
  return `Port ${port} is in use by an unknown process. Exiting.`;
}
