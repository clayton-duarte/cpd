import { mkdirSync } from "node:fs";
import { z } from "zod";
import { failure, refused } from "../lib/errors.ts";
import { run } from "../lib/exec.ts";
import { withLock } from "../lib/lock.ts";
import { beadsDir, ledgerDir, ledgerLockPath } from "../lib/paths.ts";
import { BdCreated, BdIssues, type BdIssue } from "./schema.ts";

// Contract §8: the single gateway to `bd`.

/** Statuses and types of the Ledger (trial t2 init recipe). */
export const CUSTOM_STATUSES = "draft:frozen,queued:active,running:wip,in_review:wip,failed:frozen,cancelled:frozen";
export const CUSTOM_TYPES = "workflow,job,attempt,escalation";

/** Never issued by cpd (t2 F5): they destroy history or undo Cancelled. */
const FORBIDDEN = new Set(["delete", "prune", "flatten", "gc", "compact", "reopen", "admin", "sql"]);

/** Read-only `bd` subcommands: these may run without the ledger lock. */
function isRead(args: string[]): boolean {
  const [a, b] = args;
  if (a === "dep") return b === "list";
  if (a === "comments") return b !== "add";
  return a !== undefined && ["list", "show", "ready", "children", "history", "blocked", "count", "statuses"].includes(a);
}

const baseEnv = (wf: string, env: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv => ({
  ...env,
  BEADS_DIR: beadsDir(wf),
  BEADS_ACTOR: "cpd",
  BD_DISABLE_METRICS: "1",
  DO_NOT_TRACK: "1",
  BD_NON_INTERACTIVE: "1",
});

type Call = { args: string[]; input?: string; env?: NodeJS.ProcessEnv; json?: boolean };

/** Run `bd` once. Exit code first (13 `cas_lost`, 14 `ledger_frozen`), then parse stdout with `schema`. */
async function call<S extends z.ZodType>(wf: string, c: Call, schema: S): Promise<z.infer<S>> {
  const r = await run("bd", c.json === false ? c.args : [...c.args, "--json"], {
    cwd: ledgerDir(wf),
    env: c.env ?? baseEnv(wf),
    input: c.input,
    timeoutMs: 120_000,
  });
  const tail = (s: string) => (s.length > 2000 ? s.slice(-2000) : s);
  let message = tail(r.stderr).trim();
  try {
    const e = JSON.parse(r.stdout);
    if (e && typeof e === "object" && typeof e.error === "string") message = e.error;
  } catch {
    // stdout is not an error object
  }
  const details = { args: c.args, code: r.code, message };
  if (r.code === 13) throw refused("cas_lost", `bd compare-and-swap lost: ${message}`, details);
  if (r.code === 14) throw refused("ledger_frozen", `ledger is frozen: ${message}`, details);
  if (r.code !== 0) {
    const how = r.signal ? `was killed by ${r.signal}` : `exited ${r.code}`;
    throw failure("bd_failed", `bd ${c.args.slice(0, 2).join(" ")} ${how}: ${message}`, { ...details, signal: r.signal });
  }
  if (c.json === false) return undefined as z.infer<S>;
  let raw: unknown;
  try {
    raw = JSON.parse(r.stdout);
  } catch {
    throw failure("bad_tool_output", "bd printed invalid JSON", { ...details, stdout: tail(r.stdout) });
  }
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    throw failure("bad_tool_output", "bd output does not match the expected shape", {
      ...details,
      issues: parsed.error.issues,
    });
  }
  return parsed.data;
}

/** Read access: unlocked, read-only subcommands only. */
export type ReadLedger = {
  readonly wf: string;
  read<S extends z.ZodType>(args: string[], schema: S): Promise<z.infer<S>>;
};
/** Write access: only handed out while `locks/ledger.lock` is held. */
export type Ledger = ReadLedger & {
  write<S extends z.ZodType = z.ZodUnknown>(args: string[], schema?: S, input?: string): Promise<z.infer<S>>;
};

export function readLedger(wf: string): ReadLedger {
  return {
    wf,
    async read(args, schema) {
      if (!isRead(args)) throw failure("bd_not_read_only", `bd ${args[0]} is not a read-only call`, { args });
      return call(wf, { args }, schema);
    },
  };
}

/** Run `fn` holding the Workflow's ledger lock (not re-entrant: never nest). */
export function withLedger<T>(wf: string, purpose: string, fn: (l: Ledger) => Promise<T>): Promise<T> {
  return withLock(ledgerLockPath(wf), purpose, () => {
    const l: Ledger = {
      wf,
      read: (args, schema) => readLedger(wf).read(args, schema),
      write(args, schema, input) {
        if (FORBIDDEN.has(args[0] ?? "")) throw refused("bd_forbidden", `cpd never runs bd ${args[0]}`, { args });
        return call(wf, { args, input }, (schema ?? z.unknown()) as z.ZodType) as Promise<never>;
      },
    };
    return fn(l);
  });
}

/** t2 init recipe: `bd init` in `ledger/` with BEADS_DIR unset, then the custom statuses and types. Idempotent. */
export async function initLedger(wf: string): Promise<void> {
  mkdirSync(ledgerDir(wf), { recursive: true });
  await withLedger(wf, "ledger init", async (l) => {
    const env = baseEnv(wf);
    delete env.BEADS_DIR;
    await call(
      wf,
      {
        args: ["init", "--init-if-missing", "--prefix", wf, "--skip-agents", "--skip-hooks", "--non-interactive"],
        env,
        json: false,
      },
      z.unknown(),
    );
    await l.write(["config", "set", "status.custom", CUSTOM_STATUSES]);
    await l.write(["config", "set", "types.custom", CUSTOM_TYPES]);
  });
}

/** All issues of one type (any status), as `bd list --all -n 0`. */
export async function listIssues(l: ReadLedger, type: string, extra: string[] = []): Promise<BdIssue[]> {
  return l.read(["list", "--all", "-n", "0", "-t", type, ...extra], BdIssues);
}

/** The Workflow issue of this Ledger (there is exactly one once initialised). */
export async function workflowIssue(l: ReadLedger): Promise<BdIssue> {
  const all = await listIssues(l, "workflow");
  if (all.length !== 1) {
    throw failure("ledger_invalid", `expected exactly one workflow issue, found ${all.length}`, { wf: l.wf });
  }
  return all[0]!;
}

/**
 * The `--metadata` argument that sets `patch` on top of `existing`. `bd --set-metadata` infers types
 * (a reason like "42" would become a number), so cpd always writes the whole object. With `once`,
 * a key that already exists is refused (write-once, contract §8).
 */
export function metadataArgs(existing: Record<string, unknown> | undefined, patch: Record<string, unknown>, once = false): string[] {
  const base = existing ?? {};
  if (once) {
    for (const k of Object.keys(patch)) {
      if (k in base) throw refused("metadata_write_once", `metadata key ${k} is already set`, { key: k });
    }
  }
  return ["--metadata", JSON.stringify({ ...base, ...patch })];
}

export async function createIssue(l: Ledger, args: string[]): Promise<string> {
  return (await l.write(["create", ...args], BdCreated)).id;
}
