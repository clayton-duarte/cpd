import { randomBytes } from "node:crypto";
import { chmodSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** contract §6.16 "The WebUI token, per server (b13)": this boot's secret, and the removal of its file. */
export interface IssuedToken {
  readonly token: string;
  readonly pid: number;
  remove(): void;
}

/**
 * Sweeps the files of dead pids in `<cpdHome>/run/webui-tokens/`.
 */
function sweepDeadTokens(tokensDir: string, currentPid: number): void {
  let entries: string[];
  try {
    entries = readdirSync(tokensDir);
  } catch {
    return;
  }
  for (const entry of entries) {
    if (!entry.endsWith(".token")) continue;
    const pidStr = entry.slice(0, -".token".length);
    const pid = parseInt(pidStr, 10);
    if (isNaN(pid) || pid <= 0) continue;
    if (pid === currentPid) continue;
    let alive = false;
    try {
      process.kill(pid, 0);
      alive = true;
    } catch (err: any) {
      if (err?.code === "EPERM") {
        alive = true;
      } else {
        alive = false;
      }
    }
    if (!alive) {
      try {
        rmSync(join(tokensDir, entry), { force: true });
      } catch {
        // ignore
      }
    }
  }
}

/**
 * Creates a random secret of 32 bytes (hex) and writes it to `<cpdHome>/run/webui-tokens/<pid>.token`, mode 0600.
 * A server writes only its own file, once, at its start, sweeps files of dead pids, and removes only its own file on close.
 * The token stays with this process, the cpd CLI and the host: it is never sent to the browser.
 */
export function writeWebuiToken(cpdHome: string, pid?: number): IssuedToken {
  const run = join(cpdHome, "run");
  const tokensDir = join(run, "webui-tokens");
  mkdirSync(tokensDir, { recursive: true });

  const actualPid = pid ?? process.pid;

  sweepDeadTokens(tokensDir, actualPid);

  const file = join(tokensDir, `${actualPid}.token`);
  const token = randomBytes(32).toString("hex");

  rmSync(file, { force: true });
  writeFileSync(file, token, { mode: 0o600 });
  chmodSync(file, 0o600);

  return {
    token,
    pid: actualPid,
    remove: () => {
      try {
        rmSync(file, { force: true });
      } catch {
        // ignore
      }
    },
  };
}
