import { execFile } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, realpathSync, statSync } from "node:fs";
import { createRequire } from "node:module";
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

/** commands.md §15.6 "Pre-validation": the entry files a package must have, and the bound on the throwaway load. */
export const ENTRY_FILES = ["extension.ts", "package.json"];
export const LOAD_TIMEOUT_MS = 10_000;

/**
 * The throwaway load, run by `node --input-type=module -e` so no file outside the adapter is needed: it loads the entry through the
 * loader (`createJiti`, as pi does: `moduleCache: false`) and never calls the default export. Exit 0 when that export is a function.
 */
const PROBE = `
const [loader, entry] = process.argv.slice(1);
try {
  const { createJiti } = await import(loader);
  const factory = await createJiti(entry, { moduleCache: false, fsCache: false }).import(entry, { default: true });
  if (typeof factory !== "function") throw new Error("the default export of extension.ts is not a function");
} catch (e) {
  console.error(e instanceof Error ? e.message : String(e));
  process.exit(1);
}
`;

/** `jiti` from pi's own installation (resolved from the real path of pi's entry script); throws when pi is not a node install that ships it. */
export function piJiti(): string {
  const entry = process.argv[1];
  if (entry === undefined) throw new Error("cannot find jiti: pi's entry script is unknown");
  try {
    return createRequire(realpathSync(entry)).resolve("jiti");
  } catch {
    throw new Error(`cannot find jiti from pi's installation (${entry})`);
  }
}

const firstLine = (text: string): string => text.split("\n").find((l) => l.trim() !== "")?.trim() ?? "";

export type Validation = {
  /** A module exporting `createJiti`. Default: {@link piJiti}. */
  loader?: string;
  timeoutMs?: number;
};

/**
 * Whether the package in `dir` may replace the running adapter: its entry files exist and a child `node` loads `extension.ts` through jiti.
 * Returns the reason it may not, or `undefined`. Never loads anything in this process.
 */
export async function validatePackage(dir: string, v: Validation = {}): Promise<string | undefined> {
  if (!ENTRY_FILES.every((f) => existsSync(join(dir, f)))) return "entry files missing";
  let loader: string;
  try {
    loader = v.loader ?? piJiti();
  } catch (e) {
    return (e as Error).message;
  }
  const timeout = v.timeoutMs ?? LOAD_TIMEOUT_MS;
  return new Promise((resolve) => {
    execFile("node", ["--input-type=module", "-e", PROBE, loader, join(dir, "extension.ts")], { cwd: dir, timeout, killSignal: "SIGKILL", encoding: "utf8" }, (err, _stdout, stderr) => {
      if (!err) return resolve(undefined);
      if ((err as { killed?: boolean }).killed) return resolve(`load timed out after ${timeout} ms`);
      resolve(firstLine(stderr) || err.message);
    });
  });
}

export type PollOptions = {
  dir: string;
  intervalMs: number;
  /** The throwaway load of the new package: the reason it must not be reloaded, or `undefined`. */
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
