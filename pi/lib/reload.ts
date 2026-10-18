import { execFile } from "node:child_process";
import { existsSync, realpathSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Wanted } from "./state.ts";

/** commands.md §15 "Reload after an install": the adapter reloads pi itself once its own directory was replaced by an install. */

/** The adapter's own command; `sendUserMessage` with `expandPromptTemplates` is what makes pi run it (docs/research/pi-reload.md, M1). */
export const RELOAD_COMMAND = "cpd-reload-after-install";
export const RELOAD_TRIGGER = `/${RELOAD_COMMAND}`;
export const DEFAULT_POLL_MS = 5000;

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
  /** The state the adapter wants to report (§15.4): `blocked` covers escalations and Plan reviews. */
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
  /** The guard at the trigger: false skips this tick, the next one tries again. */
  allowed(): boolean;
  /** Asks pi to run the reload command. */
  trigger(): void;
  /** The new package was refused: shown once per distinct reason until a poll passes. */
  onRefused(reason: string): void;
  onError(e: unknown): void;
};

export type Poll = {
  /** Clears the timer. */
  stop(): void;
  /** The command ran and did not reload (refused, or reload did nothing): the next tick may trigger again. */
  retry(): void;
};

/**
 * Polls the identity of `dir`. A changed identity that is the same on two consecutive polls (the install is finished) and whose package
 * validates triggers once; while that trigger is outstanding no second one is sent. A package that does not validate is retried at every later
 * poll. The change is not handled until the poll is replaced (a reload starts a new one that records the new identity).
 */
export function pollAdapter(o: PollOptions): Poll {
  let baseline: string | undefined;
  /** The new identity seen at the previous poll. */
  let seen: string | undefined;
  /** The identity whose package validated. */
  let validated: string | undefined;
  let refusedFor: string | undefined;
  let triggered = false;
  let checking = false;
  let stopped = false;
  let reported = false;
  const fail = (e: unknown) => {
    if (reported) return;
    reported = true;
    o.onError(e);
  };
  const tick = async () => {
    if (triggered || checking) return;
    checking = true;
    try {
      const id = identityOf(o.dir);
      if (id === undefined) {
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
        seen = id;
        return;
      }
      if (validated !== id) {
        const reason = await o.validate(o.dir);
        if (stopped || identityOf(o.dir) !== id) return; // gone, or replaced again while loading: start over
        if (reason !== undefined) {
          if (reason !== refusedFor) o.onRefused(reason);
          refusedFor = reason;
          return;
        }
        validated = id;
        refusedFor = undefined;
      }
      if (!o.allowed()) return;
      triggered = true;
      o.trigger();
      reported = false;
    } catch (e) {
      triggered = false;
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
  };
}
