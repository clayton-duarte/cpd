import { readPreviewMode } from "../../electron/preview-mode.ts";
import { toArgv } from "../cpd-argv.ts";

/**
 * The cpd protocol number this WebUI was built for (contract §5.2, §5.5, decision pro-d556).
 * Must match the protocol number cpd provides.
 */
export const EXPECTED_PROTOCOL = 27;

export interface StatusDeps {
  cpd(argv: readonly string[]): Promise<string>;
  tmux(argv: readonly string[]): Promise<string>;
  readFile(path: string): Promise<string | null>;
  readonly env: Readonly<Record<string, string | undefined>>;
  drift(root: string): Promise<DriftSummary>;
  /** The serve status pane (CONTRACT §20): the engine's Worker counts and incident state, from the serve stream. */
  readonly statusPane?: StatusPane | null;
}
export interface DriftSummary { readonly count: number; readonly oldestAt: string | null; readonly stale: boolean }
/** The serve status pane (web/CONTRACT.md §20), as the engine writes it. */
export interface StatusPane {
  readonly workers: { readonly queued: number; readonly waiting: number; readonly running: number; readonly limit: number };
  readonly incident: { readonly on: boolean; readonly started_at: string | null; readonly lead: string | null };
}
export interface Status {
  readonly runner: string | null;
  readonly preview: string | null;
  readonly incident: { readonly startedAt: string; readonly lead: string | null } | null;
  readonly workers: { readonly queued: number; readonly waiting: number; readonly tenants: number; readonly limit: number } | null;
  readonly version: string | null;
  readonly hostAlert: boolean;
  readonly mode: "production" | "preview";
  readonly drift: DriftSummary | null;
  readonly versionMismatch?: boolean;
  readonly alert?: string;
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
  // The workers counts and the incident state come from the serve status pane (CONTRACT §20): the engine's
  // own numbers off the serve stream, not a tmux scrape of the TUI's display string nor a `cpd incident.status`.
  const pane = deps.statusPane ?? null;
  const [versionInfo, runner, preview, install] = await Promise.all([
    attempt(async () => {
      const v = json(await deps.cpd(toArgv({ kind: "version" })));
      const ver = typeof v?.["version"] === "string" ? v["version"] : null;
      const proto = typeof v?.["protocol"] === "number" ? v["protocol"] : null;
      return { version: ver, protocol: proto };
    }),
    attempt(async () => {
      const d = obj(obj(json(await deps.cpd(toArgv({ kind: "config.read" })))?.["config"])?.["experimental"])?.["durable_workers"];
      return typeof d === "boolean" ? (d ? "durable" : "pi") : null;
    }),
    tmuxOpt("@cpd_preview"),
    attempt(async () => { const t = await deps.readFile(`${home}/run/cpd-install.json`); return t === null ? null : json(t); }),
  ]);
  const version = versionInfo?.version ?? null;
  const protocol = versionInfo?.protocol ?? null;
  const versionMismatch = protocol !== null && protocol !== EXPECTED_PROTOCOL;
  const workers = pane === null
    ? null
    : { queued: pane.workers.queued, waiting: pane.workers.waiting, tenants: pane.workers.running, limit: pane.workers.limit };
  const incident = pane === null || !pane.incident.on || pane.incident.started_at === null
    ? null
    : { startedAt: pane.incident.started_at, lead: pane.incident.lead };
  let drift: DriftSummary | null = null;
  if (pm.mode === "preview") {
    const d = await deps.drift(pm.checkout);
    drift = { count: d.count, oldestAt: d.oldestAt, stale: d.stale };
  }
  const hostAlert = (install?.["status"] === "skipped") || versionMismatch;
  const res: Status = { runner, preview, incident, workers, version, hostAlert, mode: pm.mode, drift };
  if (versionMismatch) {
    (res as unknown as Record<string, unknown>)["versionMismatch"] = true;
    (res as unknown as Record<string, unknown>)["alert"] = "version_skew";
  }
  return res;
}
