import { WebError } from "../../protocol/domain.ts";
import type { CpdEvent } from "./events.ts";
import { parseEventLine } from "./events.ts";
import { createStreamWatchdog } from "./liveness.ts";

export interface StreamChild {
  write(line: string): void;
  endInput(): void;
  kill(): void;
  onLine(cb: (line: string) => void): void;
  onExit(cb: (info: { code: number | null; stderr: string }) => void): void;
}
export interface StreamPanes { readonly context: unknown; readonly plan: unknown; readonly session: unknown; readonly inbox: unknown }
export interface StreamSnapshot { readonly seq: number; readonly panes: StreamPanes }
export interface StreamDeps {
  spawn(args: string[]): StreamChild;
  scheduler: import("../../protocol/runtime.ts").Scheduler;
}
export interface CockpitStream {
  start(): void;
  stop(): void;
  ready(): boolean;
  onSnapshot(cb: (s: StreamSnapshot) => void): void;
  onEvent(cb: (e: CpdEvent) => void): void;
  onError(cb: (e: Error) => void): void;
  /** Called with true at a child's hello and false when its child exits (absent on a stream that cannot tell). */
  onState?(cb: (ready: boolean) => void): void;
  /** Resolves the `value` of the plan_show frame; rejects WebError cpd_failed on an error frame; resolves "down" when not ready. */
  planShow(id: string): Promise<unknown>;
}

export const STREAM_RESTART_BASE_MS = 500;
export const STREAM_RESTART_CAP_MS = 10_000;
export const cockpitServeArgs = (): string[] => ["cockpit", "serve"];

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const bad = (message: string, details?: Record<string, unknown>): WebError => new WebError("cpd_malformed_json", message, details);

type Frame =
  | { t: "hello" }
  | { t: "snapshot"; snapshot: StreamSnapshot }
  | { t: "event"; event: CpdEvent }
  | { t: "plan_show"; id: string; value: unknown }
  | { t: "error"; id: string | null; code: string; message: string };

/** One line to one typed frame, at the edge. Throws WebError cpd_malformed_json. */
function parseFrame(line: string): Frame {
  let v: unknown;
  try { v = JSON.parse(line); } catch { throw bad("cockpit stream line is not JSON", { field: "json" }); }
  if (!isObj(v) || typeof v["t"] !== "string") throw bad("cockpit stream line is not a frame", { field: "t" });
  switch (v["t"]) {
    case "hello":
      if (v["protocol"] !== 1) throw bad("cockpit stream protocol is not 1", { field: "protocol" });
      return { t: "hello" };
    case "snapshot": {
      const panes = v["panes"];
      if (typeof v["seq"] !== "number" || !Number.isInteger(v["seq"])) throw bad("snapshot without an integer seq", { field: "seq" });
      if (!isObj(panes)) throw bad("snapshot without panes", { field: "panes" });
      for (const k of ["context", "plan", "session", "inbox"]) if (!isObj(panes[k])) throw bad(`snapshot pane ${k} is not an object`, { field: k });
      return { t: "snapshot", snapshot: { seq: v["seq"], panes: panes as unknown as StreamPanes } };
    }
    case "event":
      if (!isObj(v["event"])) throw bad("event frame without an event", { field: "event" });
      return { t: "event", event: parseEventLine(JSON.stringify(v["event"])) };
    case "plan_show":
      if (typeof v["id"] !== "string") throw bad("plan_show frame without an id", { field: "id" });
      if (!isObj(v["value"])) throw bad("plan_show frame without a value", { field: "value" });
      return { t: "plan_show", id: v["id"], value: v["value"] };
    case "error": {
      const e = v["error"];
      if (!isObj(e) || typeof e["code"] !== "string" || typeof e["message"] !== "string") throw bad("error frame without code and message", { field: "error" });
      if (v["id"] !== null && v["id"] !== undefined && typeof v["id"] !== "string") throw bad("error frame id is not a string or null", { field: "id" });
      return { t: "error", id: typeof v["id"] === "string" ? v["id"] : null, code: e["code"], message: e["message"] };
    }
    default:
      throw bad(`unknown cockpit stream frame ${v["t"]}`, { field: "t" });
  }
}

interface Pending { resolve(v: unknown): void; reject(e: Error): void }

export function createCockpitStream(deps: StreamDeps): CockpitStream {
  const snapshotCbs: ((s: StreamSnapshot) => void)[] = [];
  const eventCbs: ((e: CpdEvent) => void)[] = [];
  const errorCbs: ((e: Error) => void)[] = [];
  const stateCbs: ((ready: boolean) => void)[] = [];
  let child: StreamChild | null = null;
  let isReady = false;
  let stopped = false;
  let failures = 0;
  let nextId = 0;
  let reported = false;
  let healed = false;
  let wait: { dispose(): void } | null = null;
  let pending = new Map<string, Pending>();

  // D3 (air-d46): the serve child is watched for a live-but-silent stall, whatever `ready()` says — a child that
  // never says `hello` is a stall. The probe writes one `refresh` on the child's stdin; a stall is raised loud on
  // the error path (the renderer's cockpit.error toast), resynced, and the child restarted with the existing backoff
  // if still silent past the second deadline.
  const requestRefresh = (): void => {
    try { child?.write(`${JSON.stringify({ t: "refresh" })}\n`); } catch { /* the child is gone; its exit follows */ }
  };
  const watchdog = createStreamWatchdog({
    scheduler: deps.scheduler,
    probe: requestRefresh,
    resync: () => {
      reported = true; // the stall toast below is the one loud report; the restart must not repeat it
      fail(new WebError("cpd_failed", "the serve stream stalled: no frame answered the refresh probe", { cpdCode: "stalled" }));
      requestRefresh(); // ask the child for a fresh full snapshot (the resync)
    },
    restart: () => {
      // Kill the current child; its exit relaunches through the existing restart backoff (13.7).
      child?.kill();
    },
  });

  const fail = (e: Error): void => { for (const cb of errorCbs) cb(e); };
  const setReady = (r: boolean): void => {
    if (isReady === r) return;
    isReady = r;
    for (const cb of stateCbs) cb(r);
  };
  const downAll = (): void => {
    const all = pending;
    pending = new Map();
    for (const p of all.values()) p.resolve("down");
  };
  const onFrame = (line: string): void => {
    if (line.trim() === "") return;
    let frame: Frame;
    try { frame = parseFrame(line); } catch (e) {
      fail(e as Error);
      if (e instanceof WebError && e.details?.["field"] === "protocol") child?.kill();
      return;
    }
    watchdog.frame(frame.t === "snapshot" ? frame.snapshot.seq : undefined);
    switch (frame.t) {
      case "hello": failures = 0; healed = false; downAll(); setReady(true); watchdog.connected(); return;
      case "snapshot": for (const cb of snapshotCbs) cb(frame.snapshot); return;
      case "event": for (const cb of eventCbs) cb(frame.event); return;
      case "plan_show": {
        const p = pending.get(frame.id);
        if (p === undefined) { fail(bad("plan_show frame matches no request", { field: "id" })); return; }
        pending.delete(frame.id);
        p.resolve(frame.value);
        return;
      }
      case "error": {
        if (frame.code === "store_too_new" && !healed && (frame.id === null || pending.has(frame.id))) { heal(); return; }
        const err = new WebError("cpd_failed", frame.message, { cpdCode: frame.code });
        if (frame.id === null) { reported = true; fail(err); return; }
        const p = pending.get(frame.id);
        if (p === undefined) { fail(err); return; }
        pending.delete(frame.id);
        p.reject(err);
      }
    }
  };
  /** store_too_new: the child is older than the store; replace it at once, silently, once per hello. */
  const heal = (): void => {
    const old = child;
    child = null;
    healed = true;
    setReady(false);
    watchdog.disconnected();
    downAll();
    old?.kill();
    launch();
  };
  const launch = (): void => {
    const c = deps.spawn(cockpitServeArgs());
    child = c;
    reported = false;
    c.onLine((l) => { if (!stopped && child === c) onFrame(l); });
    c.onExit((info) => { if (!stopped && child === c) onExit(info); });
  };
  const onExit = (info: { code: number | null; stderr: string }): void => {
    child = null;
    setReady(false);
    watchdog.disconnected();
    downAll();
    if (!reported) {
      const msg = info.stderr.trim();
      fail(new WebError("cpd_failed", msg !== "" ? msg : `cpd exited ${info.code ?? "by signal"}`, { cpdCode: "no_json", exitCode: info.code }));
    }
    const delay = Math.min(STREAM_RESTART_BASE_MS * 2 ** failures, STREAM_RESTART_CAP_MS);
    failures++;
    wait = deps.scheduler.after(delay, () => { wait = null; if (!stopped) launch(); });
  };

  return {
    start() {
      if (child !== null || stopped) return;
      launch();
      // Watch the first child from the spawn: a child that never says `hello` is exactly a stall (D3).
      watchdog.connected();
    },
    stop() {
      if (stopped) return;
      stopped = true;
      wait?.dispose(); wait = null;
      const c = child;
      child = null;
      isReady = false;
      watchdog.disconnected();
      downAll();
      if (c !== null) { c.endInput(); c.kill(); }
    },
    ready: () => isReady,
    onSnapshot: (cb) => { snapshotCbs.push(cb); },
    onEvent: (cb) => { eventCbs.push(cb); },
    onError: (cb) => { errorCbs.push(cb); },
    onState: (cb) => { stateCbs.push(cb); },
    planShow(plan) {
      const c = child;
      if (!isReady || c === null) return Promise.resolve("down");
      const id = `r${++nextId}`;
      return new Promise<unknown>((resolve, reject) => {
        pending.set(id, { resolve, reject });
        try { c.write(`${JSON.stringify({ t: "plan_show", id, plan })}\n`); } catch {
          // a failed write means the child is gone: kill it; its exit follows and resolves the request down
          c.kill();
        }
      });
    },
  };
}
