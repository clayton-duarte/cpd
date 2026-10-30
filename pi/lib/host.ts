import { homedir } from "node:os";
import { join } from "node:path";
import { callCpd, toCpdError } from "./cpd.ts";
import { errorMessage, gated } from "./delivery.ts";
import { DEFAULT_TIMING, type Schedule, type Spawn, type Timing } from "./follower.ts";
import type { Exec, ModelCtx, PiApi } from "./pi.ts";
import type { Validation } from "./reload.ts";
import { appendLog, reloadLogPath } from "./reload.ts";
import { boldPhi } from "./phi.ts";
import { Rendering } from "./render.ts";
import type { Run } from "./run.ts";

export type Sleep = (ms: number, value: undefined, options: { signal?: AbortSignal; ref?: boolean }) => Promise<undefined>;

export type Options = {
  /** The environment read at `session_start` (`CPD_SESSION`, `CPD_PLAN`, `CPD_HOME`). Default: `process.env`. */
  env?: Record<string, string | undefined>;
  timing?: Partial<Timing>;
  spawn?: Spawn;
  /** How the follower's restart is scheduled. Default: `setTimeout`. */
  schedule?: Schedule;
  /** The clock of the loop guard (commands.md §15.15 L3), in ms. Default: `Date.now`. */
  now?: () => number;
  /** The directory whose replacement means an install, how often it is polled, and how the new package is validated (§15.6). Default: the adapter's own directory, every {@link DEFAULT_POLL_MS}, jiti from pi. */
  reload?: { dir?: string; intervalMs?: number; longRefusalMs?: number; now?: () => number } & Validation;
  /** Waits `ms` (aborted by `signal`), at start-up's retry of `session show` and at the shutdown wait. Default: `timers/promises` `setTimeout`, which `t.mock.timers` does not mock. */
  sleep?: Sleep;
};

/** What the adapter's concerns share: pi, the options and the one mutable activation. Each module takes it and keeps no state of its own. */
export type Host = {
  readonly pi: PiApi;
  readonly options: Options;
  readonly env: Record<string, string | undefined>;
  readonly timing: Timing;
  readonly exec: Exec;
  /** The renderer of every cpd message; its style is loaded at `session_start` (§15.9). */
  readonly rendering: Rendering;
  /** The active run, from `session_start` to `session_shutdown`. */
  run: Run | undefined;
  /** Aborted at `session_shutdown` (and by the next start): a call in flight then ends instead of outliving the run. */
  abort: AbortController;
  /** The context of `session_start`: `ctx.ui` is only handed to handlers. */
  ctx: ModelCtx | undefined;
  /** Set while the start-up `session show` is being retried (§15.2 step 1); `stop` aborts it. */
  showRetry: AbortController | undefined;
  /** The installed cpd binaries already reported as speaking another protocol (§15.2, step 0), and the error codes of a failed `--version` already reported; both once per adapter, so a follower restart repeats nothing. */
  readonly mismatched: Set<string>;
  readonly versionFailures: Set<string>;
  /** pi has no way to remove a handler or a tool: each is registered once. */
  registered: { tool: boolean; updates: boolean; reminder: boolean; firstMessage: boolean };
  call(args: string[]): ReturnType<typeof callCpd>;
  /** Delivers a failure as an error message to the Lead. */
  deliver(what: string, e: unknown, extra?: Record<string, unknown>): void;
  /** The one place every §15.1 error goes through: no turn, and shown once per (what, code) in a row. */
  report(what: string, code: string, message: string, extra?: Record<string, unknown>): void;
  /** Reports the facts of `r` as the Session's state, when `r` is the active run and reporting has begun (§15.4). */
  refresh(r: Run): void;
};

export function createHost(pi: PiApi, options: Options): Host {
  const timing: Timing = { ...DEFAULT_TIMING, ...options.timing };
  const host: Host = {
    pi,
    options,
    env: options.env ?? process.env,
    timing,
    exec: (command, args, o) => pi.exec(command, args, o),
    rendering: new Rendering(),
    run: undefined,
    abort: new AbortController(),
    ctx: undefined,
    showRetry: undefined,
    mismatched: new Set(),
    versionFailures: new Set(),
    registered: { tool: false, updates: false, reminder: false, firstMessage: false },
    call: (args) => callCpd(host.exec, args, { timeoutMs: timing.callTimeoutMs, signal: host.abort.signal }),
    deliver(what, e, extra = {}) {
      const err = toCpdError(e);
      host.report(what, err.code, err.message, { ...err.details, ...extra });
    },
    report(what, code, message, extra = {}) {
      const r = host.run;
      // A refused MCP registration is never deduplicated and never touches the memory (one report per Event or start).
      const refused = code === "mcp_register_failed";
      if (r && !refused) {
        const key = `${what}\n${code}`;
        if (r.lastError === key) return;
        r.lastError = key;
      }
      // A Lead's errors go to the log and to the Manager, never to its chat. Before a run there is no role: CPD_PLAN is a Lead's pane (§6.5).
      const id = r?.id ?? host.env.CPD_SESSION;
      const lead = !!id && (r ? r.info.role === "lead" : !!host.env.CPD_PLAN);
      // The ledger report: a Lead's, and a refused registration's for every role.
      if (id && (lead || refused)) {
        const log = (line: string) => {
          if (r) return r.log(line);
          try {
            appendLog(reloadLogPath(host.env.CPD_HOME || join(homedir(), ".cpd"), id), line);
          } catch (e) {
            host.ctx?.ui.notify(boldPhi(`φ: cannot write the adapter log: ${toCpdError(e).message}`), "error");
          }
        };
        const oneLine = (s: string) => s.replace(/\s*\n\s*/g, " ");
        if (lead) log(`error: ${oneLine(what)}: ${oneLine(code)}: ${oneLine(message)}`);
        const args = ["session", "report", "--what", what, "--code", code, "--message", message, ...(Object.keys(extra).length > 0 ? ["--details", JSON.stringify(extra)] : []), "--session", id];
        host.call(args).catch((e) => {
          const err = toCpdError(e);
          log(`report failed: ${err.code}: ${oneLine(err.message)}`);
        });
        if (lead) return;
      }
      const m = errorMessage(what, code, message, extra);
      pi.sendMessage(m.message, gated(r, m.options));
    },
    refresh(r) {
      if (r.reporting && host.run === r) r.reporter.want(r.facts.wanted());
    },
  };
  return host;
}
