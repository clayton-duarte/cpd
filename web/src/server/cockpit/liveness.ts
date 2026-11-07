// The serve-stream watchdog (web/CONTRACT.md §15.2, docs/contract.md §5.5; air-d46).
//
// A `cpd cockpit serve` child that stays alive but stops emitting (the air-n13/n14/n15 freeze) is caught here,
// whatever `ready()` says: while the stream is ready, the safety tick skips the pane refresh and the
// `events follow` fallback early-returns, which is exactly what froze the panes. The stream is silent while the
// cockpit is idle, so a bare idle timer would fire on healthy children; the watchdog elicits frames instead:
// every `STREAM_PROBE_MS` it writes one `refresh` request on the child's stdin (the child answers at once) and
// arms `STREAM_PROBE_TIMEOUT_MS`. A stall is one of two signals: the window closes with no frame at all, or the
// answering snapshot's `seq` is not greater than the last `seq` seen from this serve process. A stall is reported
// loud once and resynced; a child still silent past the second deadline is restarted with the existing backoff.
import type { Disposable, Scheduler } from "../../protocol/runtime.ts";

/** The interval of the watchdog's `refresh` probe on the serve stream; the safety tick's cadence (§15.2). */
export const STREAM_PROBE_MS = 30_000;
/** How long after a probe the watchdog waits for any frame before it reports the stall (§15.2). */
export const STREAM_PROBE_TIMEOUT_MS = 10_000;

export interface StreamWatchdogDeps {
  /** The one clock; the watchdog never uses a real timer. */
  scheduler: Scheduler;
  /** Write one `refresh` request on the serve child's stdin (13.1) and flush it. */
  probe(): void;
  /**
   * Report the stall loud once — one error on the stream's error path, so the renderer shows the cockpit error
   * toast — and request a resync of the panes by spawn (§15.2). Called once per stall.
   */
  resync(): void;
  /**
   * Kill and relaunch the serve child through the stream's existing restart backoff (13.7); its `hello` re-arms
   * the watchdog. Called when the child is still silent past the second deadline (§15.2).
   */
  restart(): void;
}

export interface StreamWatchdog {
  /**
   * The serve child is live: it was spawned, or the same child said `hello` again after its in-place re-exec. Start
   * probing and reset the `seq` baseline, so snapshot 1 of the new generation is fresh, never a regression (§15.2).
   */
  connected(): void;
  /** One frame of any `t` arrived; pass the snapshot's `seq` when the frame is a snapshot (§15.2). */
  frame(seq?: number): void;
  /** The serve child is gone: probing stops until the next child connects. */
  disconnected(): void;
}

export function createStreamWatchdog(deps: StreamWatchdogDeps): StreamWatchdog {
  const { scheduler } = deps;
  let probeTimer: Disposable | null = null;
  let deadline: Disposable | null = null;
  let restartTimer: Disposable | null = null;
  let lastSeq: number | null = null;
  let stalled = false;

  const clearProbe = (): void => { probeTimer?.dispose(); probeTimer = null; };
  const clearDeadline = (): void => { deadline?.dispose(); deadline = null; };
  const clearRestart = (): void => { restartTimer?.dispose(); restartTimer = null; };
  const clearAll = (): void => { clearProbe(); clearDeadline(); clearRestart(); };

  const scheduleProbe = (): void => {
    clearProbe();
    probeTimer = scheduler.after(STREAM_PROBE_MS, fireProbe);
  };
  const fireProbe = (): void => {
    probeTimer = null;
    deps.probe();
    clearDeadline();
    deadline = scheduler.after(STREAM_PROBE_TIMEOUT_MS, onDeadline);
  };
  const onDeadline = (): void => {
    deadline = null;
    stall();
  };
  /**
   * Report the stall loud once, request the resync, and arm the second deadline: still silent after it and the
   * child is restarted with the existing backoff.
   */
  const stall = (): void => {
    if (stalled) return;
    stalled = true;
    clearProbe();
    clearDeadline();
    deps.resync();
    clearRestart();
    restartTimer = scheduler.after(STREAM_PROBE_TIMEOUT_MS, () => {
      restartTimer = null;
      deps.restart();
    });
  };
  /** A frame answered the probe (or one arrived), so the window closes, the cadence resumes and a stall clears. */
  const healthy = (): void => {
    clearDeadline();
    clearRestart();
    stalled = false;
    scheduleProbe();
  };

  return {
    connected(): void {
      clearAll();
      lastSeq = null;
      stalled = false;
      scheduleProbe();
    },
    frame(seq?: number): void {
      // The `seq` of the answering snapshot must advance within one generation (§15.2); a non-snapshot frame answers
      // the probe but carries no `seq`.
      if (seq !== undefined) {
        if (lastSeq !== null && seq <= lastSeq) {
          stall();
          return;
        }
        lastSeq = seq;
      }
      healthy();
    },
    disconnected(): void {
      clearAll();
      lastSeq = null;
      stalled = false;
    },
  };
}
