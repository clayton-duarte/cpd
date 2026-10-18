import { statSync } from "node:fs";
import { dirname } from "node:path";
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
  /** The state the adapter wants to report (§15.4): `blocked` covers escalations, Plan reviews and `herdr:blocked` signals. */
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

export type PollOptions = {
  dir: string;
  intervalMs: number;
  /** The guard at the trigger: false skips this tick, the next one tries again. */
  allowed(): boolean;
  /** Asks pi to run the reload command. */
  trigger(): void;
  onError(e: unknown): void;
};

export type Poll = {
  /** Clears the timer. */
  stop(): void;
  /** The command ran and did not reload (refused, or reload did nothing): the next tick may trigger again. */
  retry(): void;
};

/**
 * Polls the identity of `dir`. A changed, existing directory triggers once; while that trigger is outstanding no
 * second one is sent. The change is not handled until the poll is replaced (a reload starts a new one that records the new identity).
 */
export function pollAdapter(o: PollOptions): Poll {
  let baseline: string | undefined;
  let triggered = false;
  let reported = false;
  const fail = (e: unknown) => {
    if (reported) return;
    reported = true;
    o.onError(e);
  };
  const tick = () => {
    if (triggered) return;
    try {
      const id = identityOf(o.dir);
      if (id === undefined) return;
      if (baseline === undefined) {
        baseline = id;
        return;
      }
      if (id === baseline || !o.allowed()) return;
      triggered = true;
      o.trigger();
      reported = false;
    } catch (e) {
      triggered = false;
      fail(e);
    }
  };
  try {
    baseline = identityOf(o.dir);
  } catch (e) {
    fail(e);
  }
  const timer = setInterval(tick, o.intervalMs);
  timer.unref();
  return {
    stop: () => clearInterval(timer),
    retry: () => {
      triggered = false;
    },
  };
}
