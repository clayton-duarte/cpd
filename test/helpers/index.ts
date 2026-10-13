import { spawn } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const cleanups: string[] = [];
let hooked = false;

/** Create a temp dir, removed automatically when the process exits. */
export function tmpDir(prefix = "cpd-"): string {
  const d = mkdtempSync(join(tmpdir(), prefix));
  cleanups.push(d);
  if (!hooked) {
    hooked = true;
    process.on("exit", () => {
      for (const c of cleanups) rmSync(c, { recursive: true, force: true });
    });
  }
  return d;
}

/** Env with HOME and CPD_HOME under a temp dir; PATH preserved (optionally prefixed). */
export function sandboxEnv(extra: Record<string, string> = {}, pathPrefix: string[] = []): NodeJS.ProcessEnv {
  const root = tmpDir("cpd-home-");
  const home = join(root, "home");
  mkdirSync(home);
  const path = [...pathPrefix, process.env.PATH ?? ""].join(":");
  return { PATH: path, HOME: home, CPD_HOME: join(root, "cpd"), ...extra };
}

export type FakeBin = { dir: string; path: string; logFile: string; calls(): string[][] };

/** Executable `name` in a temp bin dir: logs argv as a JSON line, then runs `script` (sh). */
export function fakeBin(name: string, script = "", binDir = tmpDir("cpd-bin-")): FakeBin {
  const logFile = join(binDir, `${name}.log`);
  const path = join(binDir, name);
  const body = `#!/bin/sh
'${process.execPath}' -e 'process.stdout.write(JSON.stringify(process.argv.slice(1))+"\\n")' -- "$@" >> '${logFile}'
${script}
`;
  writeFileSync(path, body);
  chmodSync(path, 0o755);
  return {
    dir: binDir,
    path,
    logFile,
    calls: () =>
      existsSync(logFile)
        ? readFileSync(logFile, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l) as string[])
        : [],
  };
}

export const CLI = new URL("../../bin/cpd.mjs", import.meta.url).pathname;

/** Run bin/cpd.mjs as a subprocess. `json` is the parsed stdout (undefined if not JSON). */
export function runCli(
  args: string[],
  opts: { env?: NodeJS.ProcessEnv; cwd?: string; bin?: string } = {},
): Promise<{ code: number; json: any; stderr: string; stdout: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [opts.bin ?? CLI, ...args], {
      cwd: opts.cwd ?? process.cwd(),
      env: opts.env ?? sandboxEnv(),
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout!.setEncoding("utf8").on("data", (d: string) => (stdout += d));
    child.stderr!.setEncoding("utf8").on("data", (d: string) => (stderr += d));
    child.on("error", reject);
    child.on("close", (code) => {
      let json: unknown;
      try {
        json = JSON.parse(stdout);
      } catch {
        json = undefined;
      }
      resolve({ code: code ?? -1, json, stderr, stdout });
    });
  });
}
