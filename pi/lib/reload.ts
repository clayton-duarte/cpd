import { spawn } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Wanted } from "./state.ts";

/** commands.md §15 "Reload after an install": the adapter reloads pi itself once its own directory was replaced by an install. */

/** The adapter's own command; `sendUserMessage` with `expandPromptTemplates` is what makes pi run it (docs/research/pi-reload.md, M1). */
export const RELOAD_COMMAND = "cpd-reload-after-install";
export const RELOAD_TRIGGER = `/${RELOAD_COMMAND}`;
export const DEFAULT_POLL_MS = 5000;
/** commands.md §15.6 "Log": a reload refused for longer than this (after the new package validated) is told to the User once. */
export const LONG_REFUSAL_MS = 10 * 60_000;
export const LONG_REFUSAL_NOTICE = "cpd: a new adapter is installed; reload when free (/reload)";

/** commands.md §15.6 "Log": where this Session's reload decisions are written. */
export const reloadLogPath = (cpdHome: string, session: string): string => join(cpdHome, "run", "adapter", `${session}.log`);

/** Appends `<ISO time> <line>` to `path`, creating its directory. Throws when it cannot write: the caller decides who hears of it. */
export function appendLog(path: string, line: string, now: () => Date = () => new Date()): void {
  mkdirSync(dirname(path), { recursive: true });
  appendFileSync(path, `${now().toISOString()} ${line}\n`);
}

/** The adapter directory: the parent of `pi/lib/`, which is what `scripts/install-cpd.sh` replaces as a whole. */
export const adapterDir = (): string => dirname(dirname(fileURLToPath(import.meta.url)));

/**
 * The path to watch: the value of pi's `-e`/`--extension` argument whose real path is `dir` (the link pi loaded the adapter from, which an
 * install swaps), else `dir`. A value that does not resolve (an `npm:` spec, a missing path) is skipped.
 */
export function watchedDir(argv: readonly string[], dir: string): string {
  for (let i = 0; i < argv.length - 1; i++) {
    if (argv[i] !== "-e" && argv[i] !== "--extension") continue;
    const value = argv[i + 1]!;
    try {
      if (realpathSync(value) === realpathSync(dir)) return value;
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code !== "ENOENT") throw e;
    }
  }
  return dir;
}

/** `(dev, ino, mtimeMs)` of the directory; `undefined` when it does not exist (the install's `rm` → `mv` window, not a change). */
export function identityOf(dir: string): string | undefined {
  try {
    const s = statSync(dir);
    return `${s.dev}:${s.ino}:${s.mtimeMs}`;
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return undefined;
    throw e;
  }
}

export type Guard = {
  /** pi's `ctx.isIdle()`. */
  idle: boolean;
  /** pi's `ctx.hasPendingMessages()`. */
  pending: boolean;
  /** The state the adapter wants to report (§15.4): `blocked` covers escalations, Plan reviews and pi dialogs. */
  state: Wanted["state"];
  /** `ctx.ui.getEditorText()`. */
  editorText: string;
};

export type Refusal = "busy" | "pending" | "state" | "typing";

/** Why a reload must not happen now (first reason, in the order of the contract), or `undefined` when it may. */
export function refusal(g: Guard): Refusal | undefined {
  if (!g.idle) return "busy";
  if (g.pending) return "pending";
  if (g.state !== "idle") return "state";
  if (g.editorText !== "") return "typing";
  return undefined;
}

/** commands.md §15.6 "Pre-validation": the entry files a package must have, and the bound on pi's own load of it. */
export const ENTRY_FILES = ["extension.ts", "package.json"];
/** Generous, so a loaded machine is not a failure: a timeout is a refusal that is tried again at a later poll. */
export const LOAD_TIMEOUT_MS = 60_000;

const firstLine = (text: string): string => text.split("\n").find((l) => l.trim() !== "")?.trim() ?? "";

export type Validation = {
  timeoutMs?: number;
};

/**
 * Whether the package in `dir` may replace the running adapter: its entry files exist and pi itself loads it
 * (`PI_OFFLINE=1 pi --no-extensions --no-session -e <dir> --mode rpc </dev/null`: exit 0 when the extension loads, 1 with pi's message when it does not).
 * `pi` is the one on `PATH`, which is how cpd starts the session; its agent directory is an empty scratch directory (no User settings, sessions or
 * credentials), removed afterwards. Returns the reason it may not, or `undefined`. Never loads anything in this process.
 */
export async function validatePackage(dir: string, v: Validation = {}): Promise<string | undefined> {
  if (!ENTRY_FILES.every((f) => existsSync(join(dir, f)))) return "entry files missing";
  const timeout = v.timeoutMs ?? LOAD_TIMEOUT_MS;
  const scratch = mkdtempSync(join(tmpdir(), "cpd-validate-"));
  try {
    return await new Promise<string | undefined>((resolve) => {
      const child = spawn("pi", ["--no-extensions", "--no-session", "-e", dir, "--mode", "rpc"], {
        cwd: scratch,
        env: { ...process.env, PI_OFFLINE: "1", PI_CODING_AGENT_DIR: scratch },
        stdio: ["ignore", "ignore", "pipe"],
        detached: true, // its own process group, so a timeout kills what pi started too
      });
      let stderr = "";
      child.stderr.setEncoding("utf8");
      child.stderr.on("data", (d: string) => (stderr += d));
      const timer = setTimeout(() => {
        try {
          process.kill(-child.pid!, "SIGKILL");
        } catch {
          child.kill("SIGKILL");
        }
        resolve(`load timed out after ${timeout} ms`);
      }, timeout);
      child.on("error", (e) => {
        clearTimeout(timer);
        resolve(`cannot run pi: ${e.message}`);
      });
      child.on("close", (code, signal) => {
        clearTimeout(timer);
        resolve(code === 0 ? undefined : firstLine(stderr) || `pi exited with ${code ?? signal}`);
      });
    });
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

export type PollOptions = {
  dir: string;
  intervalMs: number;
  /** pi's own load of the new package: the reason it must not be reloaded, or `undefined`. */
  validate(dir: string): Promise<string | undefined>;
  /** The guard at the trigger: the reason it refuses (this tick is skipped, the next one tries again), or `undefined` when it may trigger. */
  refusal(): string | undefined;
  /** Asks pi to run the reload command. */
  trigger(): void;
  /** The new package was refused: shown once per distinct reason until a poll passes. */
  onRefused(reason: string): void;
  onError(e: unknown): void;
  /** One line per decision (commands.md §15.6 "Log"). */
  log(line: string): void;
  /** The validated package has been refused for more than `longRefusalMs`: called once per install. */
  onLongRefusal(): void;
  longRefusalMs?: number;
  /** Milliseconds, for the long refusal. Default: `Date.now`. */
  now?: () => number;
};

export type Poll = {
  /** Clears the timer. */
  stop(): void;
  /** The command ran and did not reload (refused, or reload did nothing): the next tick may trigger again. */
  retry(): void;
  /**
   * Decides now instead of at the next tick, for a package that already validated and is waiting for pi to be free (pi just settled: a busy
   * Lead's idle gaps can be shorter than the interval). Does nothing before validation, while a trigger is outstanding, or when the directory changed again.
   */
  nudge(): void;
};

/**
 * Polls the identity of `dir`. A changed identity that is the same on two consecutive polls (the install is finished) and whose package
 * validates triggers once; while that trigger is outstanding no second one is sent. A package that does not validate is retried at every later
 * poll. The change is not handled until the poll is replaced (a reload starts a new one that records the new identity).
 * Every decision is logged; a waiting or refusing line is not repeated while it is the last one.
 */
export function pollAdapter(o: PollOptions): Poll {
  const now = o.now ?? Date.now;
  const longMs = o.longRefusalMs ?? LONG_REFUSAL_MS;
  let baseline: string | undefined;
  /** The new identity seen at the previous poll. */
  let seen: string | undefined;
  /** The identity whose package validated, and since when it has been waiting to reload. */
  let validated: string | undefined;
  let readySince = 0;
  let longTold = false;
  let refusedFor: string | undefined;
  /** The identity whose validation was logged. */
  let announced: string | undefined;
  let triggered = false;
  let checking = false;
  let stopped = false;
  let reported = false;
  let lastLine: string | undefined;
  /** A waiting or refusing line: not repeated while it is the last one. */
  const note = (line: string) => {
    if (line === lastLine) return;
    lastLine = line;
    o.log(line);
  };
  const log = (line: string) => {
    lastLine = undefined;
    o.log(line);
  };
  const fail = (e: unknown) => {
    if (reported) return;
    reported = true;
    o.onError(e);
  };
  /** The package in `dir` validated: tells the User of a long refusal, then triggers when the guard allows. */
  const attempt = (via: "tick" | "nudge") => {
    if (!longTold && now() - readySince > longMs) {
      longTold = true;
      log(`poll: not reloaded ${Math.round((now() - readySince) / 1000)} s after the package validated; told the User`);
      o.onLongRefusal();
    }
    const why = o.refusal();
    if (why !== undefined) {
      note(`poll: refused: ${why}`);
      return;
    }
    triggered = true;
    log(`poll: trigger sent (/${RELOAD_COMMAND}, at a ${via})`);
    o.trigger();
    reported = false;
  };
  const tick = async () => {
    if (triggered || checking) return;
    checking = true;
    try {
      const id = identityOf(o.dir);
      if (id === undefined) {
        if (seen !== undefined || baseline !== undefined) note("poll: the adapter directory is missing (an install in progress); waiting");
        seen = undefined; // the install's rm → mv window
        return;
      }
      if (baseline === undefined) {
        baseline = id;
        return;
      }
      if (id === baseline) {
        seen = undefined;
        return;
      }
      if (seen !== id) {
        note(seen === undefined ? `poll: the identity changed ${baseline} -> ${id}; not stable yet` : `poll: the identity changed again ${seen} -> ${id}; not stable yet`);
        seen = id;
        return;
      }
      if (validated !== id) {
        if (announced !== id) log(`poll: ${id} is stable; validating the package`);
        announced = id;
        const reason = await o.validate(o.dir);
        if (stopped || identityOf(o.dir) !== id) return; // gone, or replaced again while loading: start over
        if (reason !== undefined) {
          if (reason !== refusedFor) o.onRefused(reason);
          note(`poll: pre-validation failed: ${reason}`);
          refusedFor = reason;
          return;
        }
        log("poll: pre-validation passed");
        validated = id;
        readySince = now();
        longTold = false;
        refusedFor = undefined;
      }
      attempt("tick");
    } catch (e) {
      triggered = false;
      log(`poll: failed: ${e instanceof Error ? e.message : String(e)}`);
      fail(e);
    } finally {
      checking = false;
    }
  };
  try {
    baseline = identityOf(o.dir);
  } catch (e) {
    fail(e);
  }
  log(`poll: watching ${o.dir} (${baseline ?? "missing"}) every ${o.intervalMs} ms`);
  const timer = setInterval(() => void tick(), o.intervalMs);
  timer.unref();
  return {
    stop: () => {
      stopped = true;
      clearInterval(timer);
    },
    retry: () => {
      triggered = false;
    },
    nudge: () => {
      if (stopped || triggered || checking || validated === undefined) return;
      try {
        if (identityOf(o.dir) !== validated) return; // replaced since: the next tick starts over
        attempt("nudge");
      } catch (e) {
        triggered = false;
        log(`poll: failed: ${e instanceof Error ? e.message : String(e)}`);
        fail(e);
      }
    },
  };
}
