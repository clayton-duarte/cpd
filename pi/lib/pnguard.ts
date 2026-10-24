// commands.md §15 "Every plannotator session is tracked" (G1-G3): which plannotator calls are refused. Pure: the adapter (`adapter.ts`) applies it to a `tool_call`.
import { G3_TEXT, registryReadable } from "./pntrack.ts";

export type GuardState = {
  env: Record<string, string | undefined>;
  /** A live cpd run exists (start-up finished). */
  live: boolean;
  /** Where the registry lives (`globalThis`). */
  store: Record<string, unknown>;
};

export type GuardCall = { toolName: string; input: unknown; parentToolCallId?: string };

const OPEN_ACTIONS = new Set(["annotate", "review", "last"]);
const refuse = (reason: string) => ({ block: true, reason }) as const;
const G2 = (cause: string) => refuse(`cpd: plannotator needs a link the User can reach; refused: ${cause}`);

const loopback = (host: string): boolean => {
  const h = host.toLowerCase().replace(/^\[|\]$/g, "");
  return h === "" || h === "localhost" || h === "::1" || h === "0.0.0.0" || /^127\.\d+\.\d+\.\d+$/.test(h);
};

export function pnVerdict(state: GuardState, call: GuardCall): { block: true; reason: string } | undefined {
  const opens =
    call.toolName === "plannotator_submit_plan" ||
    (call.toolName === "plannotator" && typeof (call.input as { action?: unknown } | null)?.action === "string" && OPEN_ACTIONS.has((call.input as { action: string }).action));
  if (!opens) return undefined;
  const { env } = state;
  if (env.CPD_SESSION === undefined || env.CPD_SESSION === "") return undefined; // not a pi cpd started
  if (!state.live) return refuse("cpd: plannotator waits for the cpd adapter to start");
  if (env.PLANNOTATOR_REMOTE !== "1") return G2("PLANNOTATOR_REMOTE is not 1");
  if (loopback(env.PLANNOTATOR_URL_HOST ?? "")) return G2("PLANNOTATOR_URL_HOST is empty or a loopback name or address");
  const port = env.PLANNOTATOR_PORT ?? "";
  const isPort = (t: string) => /^\d+$/.test(t) && Number(t) >= 1 && Number(t) <= 65535;
  const [a, b, ...rest] = port.split("-");
  const ok = rest.length === 0 && (b === undefined ? isPort(a) : isPort(a) && isPort(b) && Number(a) <= Number(b));
  if (!ok) return G2("PLANNOTATOR_PORT is not a port or a range of ports");
  if (!registryReadable(state.store)) return refuse(G3_TEXT);
  return undefined;
}
