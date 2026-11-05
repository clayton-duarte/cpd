// Preview watchdog supervisor: thin wiring; every decision lives in watchdog.ts.
import { execFile, spawn } from "node:child_process";
import { appendFile, cp, mkdir, open, readFile, rename, rm, stat } from "node:fs/promises";
import { join } from "node:path";
import { createRestorePoints } from "./restore-points.ts";
import { parseHealth } from "./health.ts";
import { createWatchdog } from "./watchdog.ts";

const argv = process.argv.slice(2);
const opts = {};
let relaunchCmd = [];
for (let i = 0; i < argv.length; i++) {
  const a = argv[i];
  if (a === "--relaunch") {
    relaunchCmd = argv.slice(i + 1);
    break;
  }
  const key = { "--worktree": "worktree", "--home": "home", "--stabilize-ms": "stabilizeMs", "--crash-debounce-ms": "crashDebounceMs", "--loading-grace-ms": "loadingGraceMs" }[a];
  if (!key || i + 1 >= argv.length) {
    console.error(`preview-watchdog: bad argument ${a}`);
    process.exit(2);
  }
  opts[key] = argv[++i];
}
if (!opts.worktree || !opts.home || relaunchCmd.length === 0) {
  console.error("usage: preview-watchdog.mjs --worktree <dir> --home <cpd-home> [--stabilize-ms n] [--crash-debounce-ms n] [--loading-grace-ms n] --relaunch <cmd> [args...]");
  process.exit(2);
}
const num = (v) => {
  if (v === undefined) return undefined;
  const n = Number(v);
  if (!Number.isFinite(n) || n < 0) {
    console.error(`preview-watchdog: bad number ${v}`);
    process.exit(2);
  }
  return n;
};

const runDir = join(opts.home, "run");
const healthFile = join(runDir, "preview-health.json");
const crashLog = join(runDir, "preview-crash.log");
const watchdogLog = join(runDir, "preview-watchdog.log");
await mkdir(runDir, { recursive: true });

const execP = (file, args, o) =>
  new Promise((resolve, reject) => {
    execFile(file, args, { cwd: o.cwd, env: { ...process.env, ...o.env }, maxBuffer: 64 * 1024 * 1024 }, (err, stdout) =>
      err ? reject(err) : resolve({ stdout }),
    );
  });

const restore = createRestorePoints({
  worktree: opts.worktree,
  exec: execP,
  fs: {
    cp: (s, d) => cp(s, d, { recursive: true }),
    rm: (p) => rm(p, { recursive: true, force: true }),
    rename,
    exists: (p) => stat(p).then(() => true, () => false),
  },
});

const watchdog = createWatchdog(
  {
    now: () => Date.now(),
    readHealth: async () => {
      let text;
      try {
        text = await readFile(healthFile, "utf8");
      } catch (error) {
        if (error.code === "ENOENT") return null;
        throw error;
      }
      return parseHealth(text);
    },
    pidAlive: (pid) => {
      try {
        process.kill(pid, 0);
        return true;
      } catch (error) {
        return error.code === "EPERM";
      }
    },
    signalReload: (pid) => process.kill(pid, "SIGUSR1"),
    relaunch: async () => {
      const out = await open(watchdogLog, "a");
      const child = spawn(relaunchCmd[0], relaunchCmd.slice(1), { cwd: opts.worktree, detached: true, stdio: ["ignore", out.fd, out.fd] });
      child.unref();
      await out.close();
    },
    restore,
    log: async (entry) => {
      const line = JSON.stringify(entry);
      console.log(line);
      await appendFile(crashLog, `${line}\n`);
    },
  },
  { stabilizeMs: num(opts.stabilizeMs), crashDebounceMs: num(opts.crashDebounceMs), loadingGraceMs: num(opts.loadingGraceMs) },
);

let busy = false;
const timer = setInterval(async () => {
  if (busy) return;
  busy = true;
  try {
    await watchdog.tick();
  } catch (error) {
    console.error(`preview-watchdog: tick failed: ${error?.stack ?? error}`);
  } finally {
    busy = false;
  }
}, 500);
process.on("SIGTERM", () => {
  clearInterval(timer);
  process.exit(0);
});
