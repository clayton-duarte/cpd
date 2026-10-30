import { readdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join } from "node:path";

export type Usage = { input: number; output: number; cache_read: number; cache_write: number; cost_usd: number };
export type Row = { tool: string; calls: number; chars: number };
export type CommandRow = { key: string; calls: number; chars: number };
export type Largest = { chars: number; tool: string; command: string; file: string; entry: string };
export type RoleReport = {
  sessions: number;
  turns: number;
  tool_results: number;
  result_chars: number;
  result_tokens_est: number;
  usage: Usage;
  by_tool: Row[];
  top_commands: CommandRow[];
  largest: Largest[];
  codemode: { calls: number; output_chars: number; nested: { name: string; calls: number }[] };
};
/** The JSON report of docs/implementing.md, `Measuring tool-result bloat`. */
export type Report = {
  window: { since: string | null; until: string | null };
  files: number;
  skipped_lines: number;
  roles: { worker: RoleReport; manager: RoleReport; lead: RoleReport; other: RoleReport };
};

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const num = (v: unknown): number => (typeof v === "number" && Number.isFinite(v) ? v : 0);

type Role = keyof Report["roles"];

function emptyRole(): RoleReport {
  return {
    sessions: 0,
    turns: 0,
    tool_results: 0,
    result_chars: 0,
    result_tokens_est: 0,
    usage: { input: 0, output: 0, cache_read: 0, cache_write: 0, cost_usd: 0 },
    by_tool: [],
    top_commands: [],
    largest: [],
    codemode: { calls: 0, output_chars: 0, nested: [] },
  };
}

function roleOf(name: string | null): Role {
  if (name === null) return "other";
  if (/^\S+\s+\S+\s+a\d+$/.test(name.trim())) return "worker";
  if (name.endsWith("-manager")) return "manager";
  return "lead";
}

function bump(m: Map<string, { calls: number; chars: number }>, key: string, chars: number): void {
  const e = m.get(key) ?? { calls: 0, chars: 0 };
  e.calls += 1;
  e.chars += chars;
  m.set(key, e);
}

const byChars = <T extends { chars: number }>(rows: T[], top: number): T[] =>
  rows.sort((a, b) => b.chars - a.chars).slice(0, top);

/** Pure: the contents of pi session files in, the bloat report out. */
export function report(
  files: { name: string; text: string }[],
  options: { since?: string; until?: string; top?: number },
): Report {
  const top = options.top ?? 10;
  const since = options.since === undefined ? null : Date.parse(options.since);
  const until = options.until === undefined ? null : Date.parse(options.until);
  const roles: Report["roles"] = { worker: emptyRole(), manager: emptyRole(), lead: emptyRole(), other: emptyRole() };
  const acc = {} as Record<
    Role,
    {
      tools: Map<string, { calls: number; chars: number }>;
      cmds: Map<string, { calls: number; chars: number }>;
      nested: Map<string, number>;
      largest: Largest[];
    }
  >;
  for (const r of Object.keys(roles) as Role[]) {
    acc[r] = { tools: new Map(), cmds: new Map(), nested: new Map(), largest: [] };
  }
  let skipped = 0;
  let counted = 0;

  for (const f of files) {
    const entries: Obj[] = [];
    for (const l of f.text.split("\n")) {
      if (l.trim() === "") continue;
      try {
        const v: unknown = JSON.parse(l);
        if (isObj(v)) entries.push(v);
      } catch {
        skipped += 1;
      }
    }
    counted += 1;
    if (since !== null || until !== null) {
      const header = entries.find((e) => e["type"] === "session");
      const t = typeof header?.["timestamp"] === "string" ? Date.parse(header["timestamp"]) : NaN;
      if (Number.isNaN(t) || (since !== null && t < since) || (until !== null && t >= until)) continue;
    }
    let name: string | null = null;
    for (const e of entries) if (e["type"] === "session_info" && typeof e["name"] === "string") name = e["name"];
    const role = roleOf(name);
    const rr = roles[role];
    const a = acc[role];
    rr.sessions += 1;
    const commands = new Map<string, string>();
    for (const e of entries) {
      const m = e["message"];
      if (e["type"] !== "message" || !isObj(m) || typeof m["role"] !== "string") continue;
      if (m["role"] === "assistant") {
        rr.turns += 1;
        const u = m["usage"];
        if (isObj(u)) {
          rr.usage.input += num(u["input"]);
          rr.usage.output += num(u["output"]);
          rr.usage.cache_read += num(u["cacheRead"]);
          rr.usage.cache_write += num(u["cacheWrite"]);
          rr.usage.cost_usd += isObj(u["cost"]) ? num(u["cost"]["total"]) : 0;
        }
        if (Array.isArray(m["content"])) {
          for (const b of m["content"]) {
            if (isObj(b) && b["type"] === "toolCall" && typeof b["id"] === "string" && isObj(b["arguments"])) {
              const c = b["arguments"]["command"];
              if (typeof c === "string") commands.set(b["id"], c);
            }
          }
        }
      } else if (m["role"] === "toolResult") {
        const tool = typeof m["toolName"] === "string" ? m["toolName"] : "?";
        let chars = 0;
        if (Array.isArray(m["content"])) {
          for (const b of m["content"]) {
            if (isObj(b) && b["type"] === "text" && typeof b["text"] === "string") chars += b["text"].length;
          }
        }
        const callId = typeof m["toolCallId"] === "string" ? m["toolCallId"] : "";
        const command = commands.get(callId);
        rr.tool_results += 1;
        rr.result_chars += chars;
        bump(a.tools, tool, chars);
        const key =
          tool === "bash" ? (command === undefined ? "bash ?" : command.trim().split(/\s+/).slice(0, 2).join(" ")) : tool;
        bump(a.cmds, key, chars);
        a.largest.push({
          chars,
          tool,
          command: (command ?? "").slice(0, 80),
          file: basename(f.name),
          entry: typeof e["id"] === "string" ? e["id"] : "",
        });
        if (tool === "codemode") {
          rr.codemode.calls += 1;
          rr.codemode.output_chars += chars;
          const nc = m["nestedCalls"];
          if (isObj(nc) && Array.isArray(nc["calls"])) {
            for (const c of nc["calls"]) {
              if (isObj(c) && typeof c["name"] === "string") a.nested.set(c["name"], (a.nested.get(c["name"]) ?? 0) + 1);
            }
          }
        }
      }
    }
  }

  for (const r of Object.keys(roles) as Role[]) {
    const rr = roles[r];
    const a = acc[r];
    rr.result_tokens_est = Math.ceil(rr.result_chars / 4);
    rr.by_tool = byChars([...a.tools].map(([tool, v]) => ({ tool, ...v })), top);
    rr.top_commands = byChars([...a.cmds].map(([key, v]) => ({ key, ...v })), top);
    rr.largest = byChars(a.largest, top);
    rr.codemode.nested = [...a.nested]
      .map(([n, calls]) => ({ name: n, calls }))
      .sort((x, y) => y.calls - x.calls);
  }
  return {
    window: { since: options.since ?? null, until: options.until ?? null },
    files: counted,
    skipped_lines: skipped,
    roles,
  };
}

function text(r: Report): string {
  const lines = [
    `window: since ${r.window.since ?? "-"} until ${r.window.until ?? "-"}`,
    `files: ${r.files}  skipped_lines: ${r.skipped_lines}`,
  ];
  for (const [name, x] of Object.entries(r.roles)) {
    lines.push(
      `${name}: sessions ${x.sessions} turns ${x.turns} tool_results ${x.tool_results} result_chars ${x.result_chars} result_tokens_est ${x.result_tokens_est}`,
      `  usage: input ${x.usage.input} output ${x.usage.output} cache_read ${x.usage.cache_read} cache_write ${x.usage.cache_write} cost_usd ${x.usage.cost_usd}`,
    );
    for (const t of x.by_tool) lines.push(`  tool ${t.tool}: calls ${t.calls} chars ${t.chars}`);
    for (const c of x.top_commands) lines.push(`  command ${c.key}: calls ${c.calls} chars ${c.chars}`);
    for (const l of x.largest) lines.push(`  largest ${l.chars} ${l.tool} ${l.file} ${l.entry} ${l.command}`);
    lines.push(`  codemode: calls ${x.codemode.calls} output_chars ${x.codemode.output_chars}`);
    for (const n of x.codemode.nested) lines.push(`    nested ${n.name}: calls ${n.calls}`);
  }
  return lines.join("\n");
}

/** The command: parses options, reads `*.jsonl` one directory below the root, prints, returns the exit code. */
export function main(argv: string[], io: { out(text: string): void; err(text: string): void; home: string }): number {
  const opts: { root: string; since?: string; until?: string; top?: number; json: boolean } = {
    root: join(io.home, ".pi", "agent", "sessions"),
    json: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]!;
    if (a === "--json") {
      opts.json = true;
      continue;
    }
    if (a !== "--root" && a !== "--since" && a !== "--until" && a !== "--top") {
      io.err(`unknown option: ${a}`);
      return 2;
    }
    const v = argv[++i];
    if (v === undefined) {
      io.err(`missing value for ${a}`);
      return 2;
    }
    if (a === "--root") opts.root = v;
    else if (a === "--top") {
      if (!/^\d+$/.test(v) || Number(v) < 1) {
        io.err(`--top must be a positive integer: ${v}`);
        return 2;
      }
      opts.top = Number(v);
    } else {
      if (Number.isNaN(Date.parse(v))) {
        io.err(`${a} is not a valid time: ${v}`);
        return 2;
      }
      if (a === "--since") opts.since = v;
      else opts.until = v;
    }
  }
  const files: { name: string; text: string }[] = [];
  try {
    for (const d of readdirSync(opts.root, { withFileTypes: true })) {
      if (!d.isDirectory()) continue;
      for (const f of readdirSync(join(opts.root, d.name))) {
        if (f.endsWith(".jsonl")) files.push({ name: f, text: readFileSync(join(opts.root, d.name, f), "utf8") });
      }
    }
  } catch (e) {
    io.err(`cannot read ${opts.root}: ${e instanceof Error ? e.message : String(e)}`);
    return 2;
  }
  const r = report(files, opts);
  io.out(opts.json ? JSON.stringify(r) : text(r));
  return 0;
}

if (import.meta.main) {
  process.exitCode = main(process.argv.slice(2), {
    out: (t) => process.stdout.write(t + "\n"),
    err: (t) => process.stderr.write(t + "\n"),
    home: homedir(),
  });
}
