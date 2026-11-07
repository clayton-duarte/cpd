import { describe, expect, it } from "vitest";
import type { Disposable } from "../../protocol/runtime.ts";
import type { ServerMessage } from "../../protocol/wire.ts";
import { createStore } from "../../state/store.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import type { Cpd, FollowBoundary, FollowProcess } from "./cli.ts";
import { COALESCE_MS, createCockpitService, type CockpitUpdate, type CpdEvent } from "./events.ts";
import { createReadModel } from "./readmodel.ts";
import { createStreamWatchdog, STREAM_PROBE_MS, STREAM_PROBE_TIMEOUT_MS } from "./liveness.ts";
import type { CockpitStream, StreamSnapshot } from "./stream.ts";

// Every test here injects fake frames and a fake scheduler: no test spawns a real child, opens a port or sleeps.

/** The watchdog with an injected fake clock and recorded effects. */
function watchdogSetup() {
  const scheduler = new FakeScheduler();
  const probes: number[] = [];
  const resyncs: number[] = [];
  const restarts: number[] = [];
  const watchdog = createStreamWatchdog({
    scheduler,
    probe: () => probes.push(scheduler.now()),
    resync: () => resyncs.push(scheduler.now()),
    restart: () => restarts.push(scheduler.now()),
  });
  return { scheduler, probes, resyncs, restarts, watchdog };
}

describe("serve-stream watchdog", () => {
  it("raises a stall when no frame arrives within the deadline", () => {
    const { scheduler, probes, resyncs, watchdog } = watchdogSetup();

    // A live child that never says `hello`: `ready()` would be false, and the watchdog never consults it.
    watchdog.connected();
    expect(probes).toHaveLength(0);
    expect(resyncs).toHaveLength(0);

    // One probe goes out on the cadence; the window is still open, so nothing is loud yet.
    scheduler.advance(STREAM_PROBE_MS);
    expect(probes).toHaveLength(1);
    expect(resyncs).toHaveLength(0);

    // No frame of any `t` answered the probe: the stall is raised, once.
    scheduler.advance(STREAM_PROBE_TIMEOUT_MS);
    expect(resyncs).toHaveLength(1);

    // Probing stopped with the stall; the stall is not repeated.
    scheduler.advance(STREAM_PROBE_MS * 4);
    expect(probes).toHaveLength(1);
    expect(resyncs).toHaveLength(1);
  });

  it("raises a stall when the seq stops advancing", () => {
    const { scheduler, probes, resyncs, watchdog } = watchdogSetup();
    watchdog.connected();

    // The first probe is answered by snapshot 1: a healthy advancing `seq`.
    scheduler.advance(STREAM_PROBE_MS);
    expect(probes).toHaveLength(1);
    watchdog.frame(1);
    expect(resyncs).toHaveLength(0);

    // The next probe is answered by snapshot 1 again: the `seq` did not advance, so it is a stall.
    scheduler.advance(STREAM_PROBE_MS);
    expect(probes).toHaveLength(2);
    watchdog.frame(1);
    expect(resyncs).toHaveLength(1);
  });

  it("resyncs then restarts the child on a stall", () => {
    const { scheduler, probes, resyncs, restarts, watchdog } = watchdogSetup();
    watchdog.connected();
    scheduler.advance(STREAM_PROBE_MS);
    expect(probes).toHaveLength(1);
    expect(resyncs).toHaveLength(0);
    expect(restarts).toHaveLength(0);

    // The frame deadline closes with no frame: the stall is reported loud and a resync is requested first.
    scheduler.advance(STREAM_PROBE_TIMEOUT_MS);
    expect(resyncs).toHaveLength(1);
    expect(restarts).toHaveLength(0);

    // Still silent past the second deadline: the child is restarted.
    scheduler.advance(STREAM_PROBE_TIMEOUT_MS);
    expect(restarts).toHaveLength(1);
  });
});

// --- the lower-cursor toast (web/CONTRACT.md §15.3): the D4 surface in events.ts `apply`.

class FakeStream implements CockpitStream {
  started = 0;
  private readonly snapshotCbs: ((s: StreamSnapshot) => void)[] = [];
  private readonly eventCbs: ((e: CpdEvent) => void)[] = [];
  private readonly errorCbs: ((e: Error) => void)[] = [];
  start(): void { this.started++; }
  stop(): void { /* nothing */ }
  ready(): boolean { return true; }
  onSnapshot(cb: (s: StreamSnapshot) => void): void { this.snapshotCbs.push(cb); }
  onEvent(cb: (e: CpdEvent) => void): void { this.eventCbs.push(cb); }
  onError(cb: (e: Error) => void): void { this.errorCbs.push(cb); }
  planShow(): Promise<unknown> { return Promise.resolve("down"); }
  snapshot(panes: StreamSnapshot["panes"], seq: number): void { for (const cb of this.snapshotCbs) cb({ seq, panes }); }
}

class FakeFollow implements FollowProcess {
  onLine(): Disposable { return { dispose: () => undefined }; }
  onExit(): Disposable { return { dispose: () => undefined }; }
  kill(): void { /* not under test */ }
}

/** The four raw pane replies of one snapshot; `inboxCursor` is the inbox's `events_cursor`. */
function rawPanes(inboxCursor: number): StreamSnapshot["panes"] {
  const empty = { ok: true, events_cursor: 10 };
  return {
    context: { ...empty, contexts: [] },
    plan: { ...empty, plans: [] },
    session: { ...empty, sessions: [] },
    inbox: { ok: true, items: [], engine: { running: true }, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, events_cursor: inboxCursor },
  };
}

function fakeCpd(panes: StreamSnapshot["panes"]): Cpd {
  return {
    async json(args) {
      switch (args.join(" ")) {
        case "context list": return panes.context as Record<string, unknown>;
        case "plan list": return panes.plan as Record<string, unknown>;
        case "session list": return panes.session as Record<string, unknown>;
        case "inbox list": return panes.inbox as Record<string, unknown>;
        default: throw new Error(`unexpected ${args.join(" ")}`);
      }
    },
  };
}

const flush = async (): Promise<void> => { for (let i = 0; i < 10; i++) await Promise.resolve(); };

describe("cockpit service lower-cursor toast", () => {
  it("raises a toast for a lower cursor snapshot", async () => {
    const scheduler = new FakeScheduler();
    const stream = new FakeStream();
    const follow: FollowBoundary = { start: () => new FakeFollow() };
    const panes = rawPanes(9);
    const service = createCockpitService({ readModel: createReadModel(fakeCpd(panes)), cpd: fakeCpd(panes), follow, scheduler, stream });
    await service.start();
    const updates: CockpitUpdate[] = [];
    service.subscribe((u) => updates.push(u));

    // A first snapshot sets the held inbox cursor to 9 and is applied.
    stream.snapshot(rawPanes(9), 1);
    scheduler.advance(COALESCE_MS);
    await flush();
    expect(service.snapshot().inbox?.cursor).toBe(9);
    updates.length = 0;

    // A later snapshot carries a lower inbox cursor: it must not be dropped silently.
    stream.snapshot(rawPanes(5), 2);
    scheduler.advance(COALESCE_MS);
    await flush();

    // The pane keeps its held rows, and the drop is surfaced as one visible cockpit error (the toast).
    expect(service.snapshot().inbox?.cursor).toBe(9);
    const errors = updates.filter((u) => u.kind === "error");
    expect(errors).toHaveLength(1);

    // The error travels the existing cockpit error channel: the store's cockpit.error is set, so the toast shows.
    const store = createStore();
    store.dispatch({ type: "server", message: { type: "cockpit.error", error: errors[0]!.error } as ServerMessage });
    expect(store.getState().cockpit.error).not.toBeNull();
    service.stop();
  });
});
