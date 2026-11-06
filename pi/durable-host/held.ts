// Contract §6.15 `held.ts`: the host's memory-only map of held UI requests (4.4 rule 6), and its pure questions.
// One request per entry, oldest first (`heldList`), created through `create`, answered through `answer` (the server's
// `answerUi`), settled by the host through `resolveByHost`/`resolveAll`. The expiry is this module's seam (the impl
// Job's): `create` with an `ms` schedules an unref-ed timer that settles the request as `timeout` at `expires_at`.
import { randomUUID } from "node:crypto";
import { ProtocolError } from "./protocol.ts";

export type Held = {
  id: string;
  method: "select" | "confirm" | "input" | "editor" | "questionnaire";
  purpose: "ask_user_question" | "plan_review" | "secret_approve" | "host_approve" | "external_approve" | "mcp_login" | "other";
  created_at: number;
  expires_at?: number;
  fields: object;
  signal?: AbortSignal;
  resolve(value?: object): void;
  reject(e: unknown): void;
};

export type AnswerValue = { value?: string; confirmed?: boolean; answer?: object; cancelled?: boolean };
export type Outcome = "answered" | "cancelled" | "timeout" | "superseded" | "aborted";
export type CreateOptions = {
  ms?: number;
  signal?: AbortSignal;
  toolCallId?: string;
  onResolve(outcome: Outcome, by: string): void;
};

/** The held map's functions, as the ports that use it see them (dialogs, tools, plan-review, the server's `answerUi`). */
export type HeldApi = {
  heldCount(): number;
  heldList(): readonly Held[];
  create(method: Held["method"], fields: object, purpose: Held["purpose"], opts: CreateOptions): { id: string; promise: Promise<object | undefined> };
  answer(id: string, value: AnswerValue): { outcome: "answered" | "cancelled" };
  resolveByHost(id: string, outcome: Extract<Outcome, "timeout" | "superseded" | "aborted">): void;
  resolveAll(outcome: Extract<Outcome, "timeout" | "superseded" | "aborted">): void;
};

type Entry = Held & {
  onResolve(outcome: Outcome, by: string): void;
  timer?: ReturnType<typeof setTimeout>;
};

/** One held map: memory only (4.4 rule 6), dies with the process. The host uses the module's default instance; the tests build a fresh one per case. */
export function createHeld(): HeldApi {
  const map = new Map<string, Entry>(); // insertion order: oldest first, the §6.15 replay list

  function drop(id: string, entry: Entry): void {
    if (map.get(id) !== entry) return;
    map.delete(id);
    if (entry.timer !== undefined) clearTimeout(entry.timer);
  }

  /** Settles one entry by the host: removes it, records the outcome and resolves the promise with `undefined`. */
  function settleByHost(id: string, entry: Entry, outcome: Extract<Outcome, "timeout" | "superseded" | "aborted">, by: string): void {
    drop(id, entry);
    entry.onResolve(outcome, by);
    entry.resolve(undefined);
  }

  return {
    heldCount: () => map.size,
    heldList: () => [...map.values()],
    create(method, fields, purpose, opts) {
      const id = randomUUID();
      const created_at = Date.now();
      const expires_at = opts.ms === undefined ? undefined : created_at + opts.ms;
      let resolve!: (value?: object) => void;
      let reject!: (e: unknown) => void;
      const promise = new Promise<object | undefined>((res, rej) => {
        resolve = res;
        reject = rej;
      });
      const onSignal = (): void => settleByHost(id, entry, "superseded", "host");
      const entry: Entry = {
        id,
        method,
        purpose,
        created_at,
        ...(expires_at === undefined ? {} : { expires_at }),
        fields,
        ...(opts.signal === undefined ? {} : { signal: opts.signal }),
        resolve,
        reject,
        onResolve: opts.onResolve,
      };
      map.set(id, entry);
      // The expiry is a clock of the process (contract: "where held takes it is the impl Job's seam"): an unref-ed timer,
      // cleared with the entry, so a pending expiry never holds the process and a settled request is never touched twice.
      if (opts.ms !== undefined && opts.ms > 0) {
        entry.timer = setTimeout(() => settleByHost(id, entry, "timeout", "host"), opts.ms);
        entry.timer.unref?.();
      }
      if (opts.signal !== undefined) {
        if (opts.signal.aborted) queueMicrotask(onSignal);
        else opts.signal.addEventListener("abort", onSignal, { once: true });
      }
      return { id, promise };
    },
    answer(id, value) {
      const entry = map.get(id);
      if (entry === undefined) throw new ProtocolError("ui_unknown", `no held request ${id}`);
      const given = value.value ?? value.confirmed ?? value.answer;
      if (given !== undefined) {
        // The first answer wins: the request is removed and the promise settles with the value (4.4 rule 2).
        drop(id, entry);
        entry.onResolve("answered", "client");
        entry.resolve(given as object);
        return { outcome: "answered" };
      }
      // A cancelled value is an explicit decline, settled by the host, not by the client (4.4 rule 2, 4.6.3): the request
      // stays held (the client learned of the resolution from the ui_resolved broadcast) until the host settles it.
      return { outcome: "cancelled" };
    },
    resolveByHost(id, outcome) {
      const entry = map.get(id);
      if (entry === undefined) return;
      settleByHost(id, entry, outcome, "host");
    },
    resolveAll(outcome) {
      for (const [id, entry] of [...map]) settleByHost(id, entry, outcome, "host");
    },
  };
}

/** The module's default instance: the one map of the process the host runs (the dialogs, tools and plan-review ports use the module). */
const defaultHeld = createHeld();

export function heldCount(): number {
  return defaultHeld.heldCount();
}

export function heldList(): readonly Held[] {
  return defaultHeld.heldList();
}

export function create(method: Held["method"], fields: object, purpose: Held["purpose"], opts: CreateOptions): { id: string; promise: Promise<object | undefined> } {
  return defaultHeld.create(method, fields, purpose, opts);
}

export function answer(id: string, value: AnswerValue): { outcome: "answered" | "cancelled" } {
  return defaultHeld.answer(id, value);
}

export function resolveByHost(id: string, outcome: Extract<Outcome, "timeout" | "superseded" | "aborted">): void {
  return defaultHeld.resolveByHost(id, outcome);
}

export function resolveAll(outcome: Extract<Outcome, "timeout" | "superseded" | "aborted">): void {
  return defaultHeld.resolveAll(outcome);
}
