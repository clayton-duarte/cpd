import { execFileSync, spawn } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

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

export const CLI = fileURLToPath(new URL("../../bin/cpd.mjs", import.meta.url));

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

/**
 * A fake `wt` first on PATH while `fn` runs. `wt ... hook <name>` is logged, prints `output` and exits `code`; any other
 * `wt` call (the rebase) goes to the real `wt`. Set the fixture up before calling this. Returns what `fn` settled with and the logged hook calls.
 */
export async function withFakeWt<T>(code: number, output: string, fn: () => Promise<T>): Promise<{ result: PromiseSettledResult<T>; calls: string[][] }> {
  const real = execFileSync("sh", ["-c", "command -v wt"], { encoding: "utf8" }).trim();
  // wt's argv is `--yes -C <dir> <subcommand> ...`, so $4 is the subcommand. Only `hook` calls are logged.
  const wt = fakeBin("wt", `if [ "$4" != hook ]; then exec '${real}' "$@"; fi\nprintf '%s\\n' '${output}'\nexit ${code}`);
  const saved = process.env.PATH;
  process.env.PATH = `${wt.dir}:${saved}`;
  try {
    const [result] = await Promise.allSettled([fn()]);
    return { result, calls: wt.calls().filter((c) => c[3] === "hook") };
  } finally {
    process.env.PATH = saved;
  }
}
