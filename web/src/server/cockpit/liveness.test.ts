import { describe, expect, it } from "vitest";
import { FakeScheduler } from "../../testing/scheduler.ts";
import {
  STREAM_PROBE_MS,
  STREAM_PROBE_TIMEOUT_MS,
  createSeqGuard,
  createStreamWatchdog,
  cursorAction,
} from "./liveness.ts";

/** The watchdog with an injected fake clock: no test sleeps and none starts a process. */
function setup() {
  const scheduler = new FakeScheduler();
  const probes: number[] = [];
  const stalls: number[] = [];
  const watchdog = createStreamWatchdog({
    scheduler,
    probe: () => probes.push(scheduler.now()),
    stall: () => stalls.push(scheduler.now()),
  });
  return { scheduler, probes, stalls, watchdog };
}

describe("serve-stream watchdog", () => {
  it("detects a serve child that stops emitting and reports it loud", () => {
    const { scheduler, probes, stalls, watchdog } = setup();
    watchdog.connected();
    expect(probes).toHaveLength(0);

    // one probe is written on the cadence, and nothing is loud while the window is open
    scheduler.advance(STREAM_PROBE_MS);
    expect(probes).toHaveLength(1);
    expect(stalls).toHaveLength(0);

    // no frame of any `t` answered the probe inside the window: the stall is reported loud, once
    scheduler.advance(STREAM_PROBE_TIMEOUT_MS);
    expect(stalls).toHaveLength(1);

    // probing stops until the stream reconnects; the stall is not repeated
    scheduler.advance(STREAM_PROBE_MS * 4);
    expect(stalls).toHaveLength(1);
  });

  it("keeps the safety tick running while the stream is ready", () => {
    const { scheduler, probes, stalls, watchdog } = setup();
    watchdog.connected();

    // the stream is ready, and a healthy serve answers each probe with a frame: the tick keeps coming
    for (let i = 1; i <= 3; i++) {
      scheduler.advance(STREAM_PROBE_MS);
      expect(probes).toHaveLength(i);
      watchdog.frame();
      expect(stalls).toHaveLength(0);
    }
  });

  it("treats a seq restart at 1 after re-exec as a fresh snapshot", () => {
    const guard = createSeqGuard();
    guard.hello();
    expect(guard.accept(1)).toBe(true);
    expect(guard.accept(2)).toBe(true);
    // within one generation a lower seq is a regression, not an advance
    expect(guard.accept(1)).toBe(false);
    // a re-exec says hello again: seq starts at 1, and snapshot 1 is fresh, never a regression
    guard.hello();
    expect(guard.accept(1)).toBe(true);
  });

  it("does not silently drop a lower-cursor update without a resync", () => {
    // the first update of a pane is applied
    expect(cursorAction(null, 4)).toBe("apply");
    // an advancing or equal cursor is applied
    expect(cursorAction(4, 4)).toBe("apply");
    expect(cursorAction(4, 5)).toBe("apply");
    // a lower cursor is not dropped silently: the caller must resync
    expect(cursorAction(4, 3)).toBe("resync");
  });
});
