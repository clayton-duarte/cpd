import { readPreviewMode } from "../../electron/preview-mode.ts";
export interface StatusDeps {
  cpd(argv: readonly string[]): Promise<string>;
  tmux(argv: readonly string[]): Promise<string>;
  readFile(path: string): Promise<string | null>;
  readonly env: Readonly<Record<string, string | undefined>>;
  drift(root: string): Promise<DriftSummary>;
}
export interface DriftSummary { readonly count: number; readonly oldestAt: string | null; readonly stale: boolean }
export interface Status {
  readonly runner: string | null;
  readonly preview: string | null;
  readonly incident: { readonly startedAt: string } | null;
  readonly workers: { readonly queued: number; readonly waiting: number; readonly tenants: number; readonly limit: number } | null;
  readonly version: string | null;
  readonly hostAlert: boolean;
  readonly mode: "production" | "preview";
  readonly drift: DriftSummary | null;
}

const obj = (v: unknown): Record<string, unknown> | null => (typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : null);
async function attempt<T>(f: () => Promise<T | null>): Promise<T | null> {
  try { return await f(); } catch { return null; }
}
const json = (text: string): Record<string, unknown> | null => { try { return obj(JSON.parse(text)); } catch { return null; } };

export async function readStatus(deps: StatusDeps): Promise<Status> {
  const pm = readPreviewMode(deps.env);
  const home = process.env["CPD_HOME"] ?? `${process.env["HOME"] ?? ""}/.cpd`;
  const tmuxOpt = (name: string): Promise<string | null> => attempt(async () => (await deps.tmux(["show-options", "-qv", name])).trim() || null);
  const [version, incident, runner, workers, preview, install] = await Promise.all([
    attempt(async () => { const v = json(await deps.cpd(["--version"]))?.["version"]; return typeof v === "string" ? v : null; }),
    attempt(async () => {
      const o = json(await deps.cpd(["incident", "show"]));
      const at = obj(o?.["incident"])?.["started_at"];
      return o?.["on"] === true && typeof at === "string" ? { startedAt: at } : null;
    }),
    attempt(async () => {
      const d = obj(obj(json(await deps.cpd(["config", "show"]))?.["config"])?.["experimental"])?.["durable_workers"];
      return typeof d === "boolean" ? (d ? "durable" : "pi") : null;
    }),
    tmuxOpt("@cpd_workers").then((raw) => {
      const m = raw === null ? null : /^workers (\d+)\u{f0798}(\d+)\u{f0798}(\d+)\/(\d+)$/u.exec(raw);
      return m === null ? null : { queued: Number(m[1]), waiting: Number(m[2]), tenants: Number(m[3]), limit: Number(m[4]) };
    }),
    tmuxOpt("@cpd_preview"),
    attempt(async () => { const t = await deps.readFile(`${home}/run/cpd-install.json`); return t === null ? null : json(t); }),
  ]);
  let drift: DriftSummary | null = null;
  if (pm.mode === "preview") {
    const d = await deps.drift(pm.checkout);
    drift = { count: d.count, oldestAt: d.oldestAt, stale: d.stale };
  }
  return { runner, preview, incident, workers, version, hostAlert: install?.["status"] === "skipped", mode: pm.mode, drift };
}
