// commands.md §15 "Every plannotator session is tracked": mirrors plannotator's review registry into `cpd review open|close|list`.

/** The key plannotator (@plannotator/pi-extension 0.28.17, plannotator-tool-host.ts:182) keeps its process-wide review registry under, on `globalThis`. */
export const REGISTRY_KEY = "__plannotatorPiReviewRegistry_v1";

/** The registry's methods (plannotator-tool-host.ts:190). */
const METHODS = ["add", "remove", "openFor", "openAll", "listText", "close"] as const;
const KINDS = new Set(["annotate", "review", "last", "plan"]);

/** What cpd needs of one open review. */
export type Open = { id: string; kind: string; subject: string; port: string };

/** The refusal text of G3 (§15): the cause and the fix. */
export const G3_TEXT =
  "cpd cannot track plannotator: its review registry changed; plannotator was upgraded and its review registry changed: the pin in pi-config's settings.json and cpd's parser move together";

type Registry = { openFor(owner: string | undefined): unknown };

/** The registry under {@link REGISTRY_KEY}, or undefined when absent; throws when it is not of the 0.28.17 shape. */
export function readRegistry(store: Record<string, unknown>): Registry | undefined {
  const r = store[REGISTRY_KEY];
  if (r === undefined) return undefined;
  if (typeof r !== "object" || r === null || METHODS.some((m) => typeof (r as Record<string, unknown>)[m] !== "function")) throw new Error(G3_TEXT);
  return r as Registry;
}

/** True when the registry is present and of the known shape. */
export const registryReadable = (store: Record<string, unknown>): boolean => {
  try {
    return readRegistry(store) !== undefined;
  } catch {
    return false;
  }
};

/** Parses the reviews a registry holds for `owner`; throws on an entry that is not of the 0.28.17 shape. */
function parseOpen(registry: Registry, owner: string): Open[] {
  const list = registry.openFor(owner);
  if (!Array.isArray(list)) throw new Error(G3_TEXT);
  return list.map((e: unknown) => {
    const r = (typeof e === "object" && e !== null ? e : {}) as Record<string, unknown>;
    if (typeof r.id !== "string" || !/^pn-[0-9a-f]+$/.test(r.id)) throw new Error(G3_TEXT);
    if (typeof r.kind !== "string" || !KINDS.has(r.kind)) throw new Error(G3_TEXT);
    if (typeof r.subject !== "string" || r.subject === "" || typeof r.url !== "string") throw new Error(G3_TEXT);
    let port: string;
    try {
      port = new URL(r.url).port;
    } catch {
      throw new Error(G3_TEXT);
    }
    if (port === "") throw new Error(G3_TEXT);
    return { id: r.id, kind: r.kind, subject: r.subject.slice(0, 200), port };
  });
}

export type Tracker = {
  /** At session_start: closes as `lost` the open rows the registry lacks. */
  start(): Promise<void>;
  /** Mirrors the registry into `cpd review open` / `close`, running cpd only when it differs from the last view. */
  poll(): Promise<void>;
};

export type TrackerOptions = {
  /** `CPD_SESSION`; undefined: the adapter is not active and nothing runs. */
  session: string | undefined;
  /** Where the registry lives (`globalThis`). */
  store: Record<string, unknown>;
  /** Runs `cpd <argv>` and returns its stdout. */
  cpd: (argv: string[]) => Promise<string>;
  /** Reports a refusal (a registry of the wrong shape), once. */
  report: (message: string) => void;
};

export function createTracker(o: TrackerOptions): Tracker {
  const session = o.session === undefined || o.session === "" ? undefined : o.session;
  let known = new Set<string>(); // the ids cpd has as open, as far as this tracker knows
  let reported = false;
  let chain: Promise<unknown> = Promise.resolve();

  const run = async (argv: string[]): Promise<Record<string, unknown>> => {
    const json: unknown = JSON.parse(await o.cpd(argv));
    if (typeof json !== "object" || json === null || (json as { ok?: unknown }).ok !== true) throw new Error(`cpd ${argv.join(" ")} failed: ${JSON.stringify(json)}`);
    return json as Record<string, unknown>;
  };

  /** The registry's reviews of this Session; undefined when it is absent (`absent`) or refused (reported once). */
  const view = (): { open: Open[] } | "absent" | "refused" => {
    try {
      const registry = readRegistry(o.store);
      return registry === undefined ? "absent" : { open: parseOpen(registry, session!) };
    } catch (e) {
      if (!reported) {
        reported = true;
        o.report(e instanceof Error ? e.message : String(e));
      }
      return "refused";
    }
  };

  const sync = async (open: Open[]): Promise<void> => {
    const now = new Map(open.map((r) => [r.id, r]));
    let failure: unknown;
    for (const r of open) {
      if (known.has(r.id)) continue;
      try {
        await run(["review", "open", "--session", session!, "--pn", r.id, "--kind", r.kind, "--subject", r.subject, "--port", r.port]);
        known.add(r.id);
      } catch (e) {
        failure = e;
      }
    }
    for (const id of [...known]) {
      if (now.has(id)) continue;
      try {
        await run(["review", "close", "--session", session!, "--pn", id, "--ended", "decided"]);
        known.delete(id);
      } catch (e) {
        failure = e;
      }
    }
    if (failure !== undefined) throw failure;
  };

  const serial = <T>(f: () => Promise<T>): Promise<T> => {
    const next = chain.then(f, f);
    chain = next.catch(() => undefined);
    return next;
  };

  return {
    start: () =>
      serial(async () => {
        if (session === undefined) return;
        const v = view();
        if (v === "refused") return;
        const open = v === "absent" ? [] : v.open;
        const listed = await run(["review", "list", "--session", session]);
        const rows = Array.isArray(listed.reviews) ? listed.reviews : [];
        const live = new Set(open.map((r) => r.id));
        known = new Set();
        for (const row of rows) {
          const id = (row as { pn_id?: unknown } | null)?.pn_id;
          if (typeof id !== "string") continue;
          if (live.has(id)) known.add(id);
          else await run(["review", "close", "--session", session, "--pn", id, "--ended", "lost"]);
        }
        await sync(open);
      }),
    poll: () =>
      serial(async () => {
        if (session === undefined) return;
        const v = view();
        if (v === "refused") return;
        await sync(v === "absent" ? [] : v.open);
      }),
  };
}
