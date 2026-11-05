export interface CostFile { readonly text: string; readonly mtimeMs: number; readonly size: number }
export interface CostDeps {
  cpd(argv: readonly string[]): Promise<string>;
  readFile(path: string): Promise<CostFile | null>;
  /** The cumulative cost of a durable session's host store, `null` when it has no store. */
  durableCost?(sessionId: string): Promise<{ cost: number | null; stamp: string } | null>;
}

const obj = (v: unknown): Record<string, unknown> | null => (typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : null);
const arr = (v: unknown): Record<string, unknown>[] => (Array.isArray(v) ? v.map(obj).filter((o): o is Record<string, unknown> => o !== null) : []);
const str = (v: unknown): string | null => (typeof v === "string" ? v : null);

function costOf(u: unknown): number {
  const t = obj(obj(u)?.["cost"])?.["total"];
  if (typeof t !== "number") throw new Error("usage.cost.total");
  return t;
}
/** Sum per the core rule: throws when the file is malformed. */
function parseCost(text: string): number {
  const lines = text.split("\n").filter((l) => l.trim() !== "");
  let header: number | null = null;
  let forked = false;
  let sum = 0;
  lines.forEach((line, i) => {
    const o = obj(JSON.parse(line));
    const type = str(o?.["type"]);
    const ts = Date.parse(str(o?.["timestamp"]) ?? "");
    if (o === null || type === null || Number.isNaN(ts)) throw new Error("line");
    if (i === 0) {
      if (type !== "session") throw new Error("header");
      header = ts;
      forked = typeof o["parentSession"] === "string";
      return;
    }
    const m = obj(o["message"]);
    const counts = type === "message" ? m?.["role"] === "assistant" : ["usage", "compaction", "branch_summary"].includes(type) && obj(o["usage"]) !== null;
    if (!counts) return;
    const c = costOf(type === "message" ? m?.["usage"] : o["usage"]);
    if (forked && header !== null && ts < header) return;
    sum += c;
  });
  if (header === null) throw new Error("empty");
  return sum;
}

interface Cached { mtimeMs: number; size: number; cost: number | null }
const cache = new Map<string, Cached>();
async function fileCost(deps: CostDeps, path: string | null): Promise<number | null> {
  if (path === null) return null;
  const f = await deps.readFile(path);
  if (f === null) return null;
  const hit = cache.get(path);
  if (hit !== undefined && hit.mtimeMs === f.mtimeMs && hit.size === f.size) return hit.cost;
  let cost: number | null;
  try { cost = parseCost(f.text); } catch { cost = hit?.cost ?? null; }
  cache.set(path, { mtimeMs: f.mtimeMs, size: f.size, cost });
  return cost;
}
interface DurableCached { stamp: string; cost: number | null }
const durableCache = new Map<string, DurableCached>();
async function sessionCost(deps: CostDeps, s: Record<string, unknown>): Promise<number | null> {
  const id = str(s["id"]);
  if (s["runner"] !== "durable" || id === null || deps.durableCost === undefined) return fileCost(deps, str(s["file"]));
  const d = await deps.durableCost(id);
  if (d === null) { durableCache.delete(id); return null; }
  const hit = durableCache.get(id);
  if (hit !== undefined && hit.stamp === d.stamp) return hit.cost;
  durableCache.set(id, { stamp: d.stamp, cost: d.cost });
  return d.cost;
}
const total = (xs: (number | null)[]): number | null => (xs.some((x) => x !== null) ? xs.reduce<number>((a, x) => a + (x ?? 0), 0) : null);
const PAGE = 5000;

export async function readCosts(deps: CostDeps): Promise<Record<string, number>> {
  const contexts = arr(obj(JSON.parse(await deps.cpd(["context", "list"])))?.["contexts"]);
  const sessions = arr(obj(JSON.parse(await deps.cpd(["session", "list", "--archived"])))?.["sessions"]);
  const stored = new Map<string, number>();
  let since = 0;
  for (;;) {
    const r = obj(JSON.parse(await deps.cpd(["events", "list", "--kind", "attempt-ended", "--since", String(since), "--limit", String(PAGE)])));
    const events = arr(r?.["events"]);
    for (const e of events) {
      const c = obj(e["data"])?.["cost_usd"];
      if (typeof c === "number") stored.set(`${str(e["plan"])}/${str(e["attempt"])}`, c);
    }
    const next = r?.["next"];
    if (events.length < PAGE || typeof next !== "number") break;
    since = next;
  }
  const workers = sessions.filter((s) => s["role"] === "worker" && str(s["attempt"]) !== null);
  const out: Record<string, number> = {};
  for (const c of contexts) {
    const parts: (number | null)[] = [];
    for (const p of arr(c["plans"])) {
      const pid = str(p["id"]);
      if (pid === null) continue;
      const lead = await Promise.all(sessions.filter((s) => s["role"] === "lead" && s["plan"] === pid).map((s) => sessionCost(deps, s)));
      const keys = new Set<string>([...stored.keys()].filter((k) => k.startsWith(`${pid}/`)));
      for (const w of workers) if (w["plan"] === pid) keys.add(`${pid}/${str(w["attempt"])}`);
      const atts = await Promise.all([...keys].map((k) => {
        const s = stored.get(k);
        if (s !== undefined) return Promise.resolve(s);
        const w = workers.find((x) => `${str(x["plan"])}/${str(x["attempt"])}` === k);
        return fileCost(deps, w === undefined ? null : str(w["file"]));
      }));
      const t = total([...lead, ...atts]);
      if (t !== null) { out[pid] = t; parts.push(t); }
    }
    const mgr = await Promise.all(sessions.filter((s) => s["role"] === "manager" && s["context"] === c["id"]).map((s) => sessionCost(deps, s)));
    const id = str(c["id"]);
    const t = total([...parts, ...mgr]);
    if (id !== null && t !== null) out[id] = t;
  }
  return out;
}

/** Round half to even at `d` decimals, as Rust's formatter does on exact ties. */
function fixed(n: number, d: number): string {
  const scale = 10 ** d;
  const x = n * scale;
  const fl = Math.floor(x);
  const r = x - fl === 0.5 ? (fl % 2 === 0 ? fl : fl + 1) : Math.round(x);
  return (r / scale).toFixed(d);
}
export function formatCost(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "$0.000";
  if (n < 10) return `$${fixed(n, 3)}`;
  if (n < 100) return `$${fixed(n, 2)}`;
  if (n < 1000 && Number(fixed(n, 1)) < 1000) return `$${fixed(n, 1)}`;
  const k = n / 1000;
  return `$${k < 10 ? fixed(k, 2) : k < 100 ? fixed(k, 1) : fixed(k, 0)}k`;
}
