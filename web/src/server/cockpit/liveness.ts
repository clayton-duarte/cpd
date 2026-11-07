// The serve-stream watchdog and the freshness guards of the WebUI data path
// (contract §5.5 "The WebUI transport and the serve-stream watchdog"; ADR 0015).
//
// A serve child that stays alive but stops emitting (the air-n13/n14/n15 freeze) is caught by the
// probe watchdog: it writes a `refresh` request on the serve's stdin every STREAM_PROBE_MS and
// reports the stall loud when no frame of any `t` answers it inside STREAM_PROBE_TIMEOUT_MS. The
// watchdog runs whatever the stream's `ready()` says: readiness is not its gate.
//
// The snapshot `seq` counts from 1 per serve process, and a re-exec starts at 1 again: a `hello`
// starts a new generation and resets the baseline, so snapshot 1 after a re-exec is accepted, never
// read as a regression. A pane update whose cursor is lower than the held one is never dropped
// silently: it is a resync, not an apply.

import type { Scheduler } from "../../protocol/runtime.ts";

/** The interval of the watchdog's `refresh` probe on the serve stream (contract §5.5). */
export const STREAM_PROBE_MS = 30_000;
/** How long after a probe the watchdog waits for any frame before it reports the stall (contract §5.5). */
export const STREAM_PROBE_TIMEOUT_MS = 10_000;

export interface StreamWatchdogDeps {
  scheduler: Scheduler;
  /** Write one `refresh` request on the serve child's stdin. */
  probe(): void;
  /** Report the stall loud: one error on the stream's error path. */
  stall(): void;
}

export interface StreamWatchdog {
  /** The stream connected (a serve child's `hello`): start probing this generation. */
  connected(): void;
  /** Any frame of any `t` arrived: inside a probe window it answers the probe. */
  frame(): void;
  /** The stream disconnected (a child exit, or `stop`): stop probing. */
  disconnected(): void;
}

/** The serve-stream watchdog. The real probe/timer logic is the implementation Job's. */
export function createStreamWatchdog(deps: StreamWatchdogDeps): StreamWatchdog {
  void deps;
  return { connected() {}, frame() {}, disconnected() {} };
}

/** The snapshot `seq` of one serve process: it counts from 1, and a `hello` starts a new generation. */
export interface SeqGuard {
  /** A `hello`: a new generation, the baseline cleared (a re-exec restarts `seq` at 1). */
  hello(): void;
  /** A snapshot `seq`: true when it advances this generation (the first of a generation always does). */
  accept(seq: number): boolean;
}

/** The seq guard. A re-exec's snapshot 1 is fresh, never a regression. */
export function createSeqGuard(): SeqGuard {
  return { hello() {}, accept() { return false; } };
}

/** What to do with a pane update: `apply` when its cursor advances (or is the first), else `resync`. */
export type CursorAction = "apply" | "resync";

/** A pane update's cursor against the held one; a lower cursor is a resync, never a silent drop. */
export function cursorAction(_held: number | null, _next: number): CursorAction {
  return "apply";
}
