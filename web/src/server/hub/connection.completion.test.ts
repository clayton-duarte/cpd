import { describe, expect, it } from "vitest";
import type { SessionRef, TranscriptItem } from "../../protocol/domain.ts";
import type { ServerMessage, SessionServerMessage } from "../../protocol/wire.ts";
import { createHubConnection, type HubDeps } from "./connection.ts";

type Emit = Parameters<HubDeps["backends"]["pi"]["attach"]>[1];
type Resolved = ReturnType<HubDeps["registry"]["resolve"]>;
type Handle = Awaited<ReturnType<HubDeps["backends"]["pi"]["attach"]>>;

const flush = async (): Promise<void> => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};

const leadRef: SessionRef = { id: "lead-1", key: "c:p", runner: "pi", role: "lead", state: "idle", title: null };
const item: TranscriptItem = { kind: "user", id: "u1", text: "hi", attachments: [] };
const completion = { label: "safe_to_archive", text: "Safe to archive · 90%", score: 90, tone: "done" } as const;

const setup = (withCompletion = true, gate?: { release: () => void; promise: Promise<void> }) => {
  const sent: ServerMessage[] = [];
  const emits: Emit[] = [];
  const submits: unknown[] = [];
  const disposedHandles: number[] = [];
  const tracks: Array<{ ref: SessionRef; send: (m: SessionServerMessage) => void; observed: SessionServerMessage[]; disposed: number }> = [];
  const backend = {
    attach: async (_s: Resolved, emit: Emit): Promise<Handle> => {
      emits.push(emit);
      if (gate !== undefined) await gate.promise;
      return {
        capabilities: {},
        submit: (i: unknown) => { submits.push(i); return Promise.resolve(); },
        dispose: () => { disposedHandles.push(1); return Promise.resolve(); },
      } as unknown as Handle;
    },
  };
  const registry = { resolve: (id: string) => ({ ref: { ...leadRef, id }, file: "/l.jsonl", tierOptions: [] }) as unknown as Resolved };
  const deps: HubDeps = {
    cockpit: {} as HubDeps["cockpit"],
    registry,
    backends: { pi: backend, durable: backend } as unknown as HubDeps["backends"],
    send: (m) => sent.push(m),
    ...(withCompletion
      ? {
          completion: {
            track: (ref, send) => {
              const rec = { ref, send, observed: [] as SessionServerMessage[], disposed: 0 };
              tracks.push(rec);
              return { observe: (m) => { rec.observed.push(m); }, dispose: () => { rec.disposed += 1; } };
            },
          },
        }
      : {}),
  };
  return { hub: createHubConnection(deps), sent, emits, submits, tracks, disposedHandles };
};
const attach = (id = "lead-1") => ({ type: "session.attach", sessionId: id });
const win: SessionServerMessage = { type: "session.window", sessionId: "lead-1", mode: "tail", window: { items: [item], hasEarlier: false, cursor: null } };
const comp: SessionServerMessage = { type: "session.completion", sessionId: "lead-1", completion };

describe("hub completion wiring", () => {
  it("a Lead attach sends session.completion after the backend messages", async () => {
    const t = setup();
    await t.hub.handle(attach());
    expect(t.tracks).toHaveLength(1);
    expect(t.tracks[0]?.ref.id).toBe("lead-1");
    t.emits[0]?.(win);
    expect(t.tracks[0]?.observed).toEqual([win]);
    expect(t.sent).toEqual([win]);
    t.tracks[0]?.send(comp);
    expect(t.sent).toEqual([win, comp]);
  });
  it("session.completion is not sent after detach", async () => {
    const t = setup();
    await t.hub.handle(attach());
    await t.hub.handle({ type: "session.detach", sessionId: "lead-1" });
    expect(t.tracks[0]?.disposed).toBe(1);
    t.tracks[0]?.send(comp);
    expect(t.sent).toEqual([]);
  });
  it("session.completion is not sent after close", async () => {
    const t = setup();
    await t.hub.handle(attach());
    await t.hub.close();
    expect(t.tracks[0]?.disposed).toBe(1);
    t.tracks[0]?.send(comp);
    expect(t.sent).toEqual([]);
  });
  it("a stale attach never sends session.completion", async () => {
    let release: () => void = () => undefined;
    const promise = new Promise<void>((r) => { release = r; });
    const t = setup(true, { release, promise });
    const first = t.hub.handle(attach("lead-1"));
    await flush();
    const second = t.hub.handle({ type: "session.detach", sessionId: "lead-1" });
    await flush();
    release();
    await Promise.all([first, second]);
    expect(t.tracks.length).toBeGreaterThanOrEqual(1);
    for (const tr of t.tracks) {
      expect(tr.disposed).toBeGreaterThanOrEqual(1);
      tr.send(comp);
    }
    expect(t.sent.filter((m) => m.type === "session.completion")).toEqual([]);
  });
  it("without a completion dep the hub is unchanged", async () => {
    const t = setup(false);
    await t.hub.handle(attach());
    t.emits[0]?.(win);
    expect(t.tracks).toHaveLength(0);
    expect(t.sent).toEqual([win]);
    await t.hub.close();
    const control = setup();
    await control.hub.handle(attach());
    expect(control.tracks).toHaveLength(1); // control: with the dep a tracker is created
    control.emits[0]?.(win);
    expect(control.sent).toEqual([win]); // and the forwarded messages are the same
  });
  it("a pending classification does not delay a submit", async () => {
    const t = setup();
    await t.hub.handle(attach());
    t.emits[0]?.(win); // the tracker's computation is pending forever: it never sends
    await t.hub.handle({ type: "session.submit", sessionId: "lead-1", clientSubmissionId: "c1", text: "go", images: [], whenBusy: "followUp" });
    expect(t.submits).toHaveLength(1);
    expect(t.sent.filter((m) => m.type === "session.error")).toEqual([]);
    expect(t.tracks[0]?.disposed).toBe(0);
  });
});
