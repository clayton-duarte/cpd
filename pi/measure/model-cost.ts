import { existsSync, readdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import { DatabaseSync } from "node:sqlite";

export interface ModelCostOptions {
  json?: boolean;
  root?: string;
  durableRoot?: string;
  bifrostCsv?: string;
  since?: string;
  until?: string;
  diffPrefix?: boolean;
}

export interface SpendGroup {
  turns: number;
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
  prompt_tokens: number;
  cache_hit_rate: number | null;
  cost_usd: number;
}

export interface CacheBreakDiff {
  scope: "messages" | "system+messages";
  prefix_changed: boolean;
  index: number;
  char: number;
  tokens_from: number;
  prev_tokens_to: number;
  cur_tokens_to: number;
}

export interface CacheBreak {
  source: string;
  session: string;
  time: string;
  model: string;
  role: string;
  class: "to_zero" | "partial";
  prev_cache_read: number;
  cache_read: number;
  gap_s: number;
  idle_gap: boolean;
  cause: string | null;
  diff?: CacheBreakDiff;
}

export interface ReconciliationRow {
  utc_day: string;
  model: string;
  local_cost_usd: number;
  bifrost_cost_usd: number;
  delta_usd: number;
  ratio: number | null;
  local_cache_read: number;
  bifrost_cached_read: number;
}

export interface ModelCostReport {
  window: { since: string | null; until: string | null };
  files: { pi: number; durable: number; bifrost: number };
  skipped_lines: number;
  zero_usage_turns: number;
  totals: SpendGroup;
  by_role: {
    worker: SpendGroup;
    manager: SpendGroup;
    lead: SpendGroup;
    other: SpendGroup;
  };
  by_model: Record<string, SpendGroup>;
  by_day: Record<string, SpendGroup>;
  cache_breaks: CacheBreak[];
  reconciliation: ReconciliationRow[];
}

type Role = "worker" | "manager" | "lead" | "other";

function emptySpendGroup(): SpendGroup {
  return {
    turns: 0,
    input: 0,
    output: 0,
    cache_read: 0,
    cache_write: 0,
    prompt_tokens: 0,
    cache_hit_rate: null,
    cost_usd: 0,
  };
}

function roleOf(name: string | null): Role {
  if (name === null) return "other";
  if (/^\S+\s+\S+\s+a\d+$/.test(name.trim())) return "worker";
  if (name.endsWith("-manager")) return "manager";
  return "lead";
}

function addTurnToSpendGroup(
  g: SpendGroup,
  input: number,
  output: number,
  cacheRead: number,
  cacheWrite: number,
  cost: number,
): void {
  g.turns += 1;
  g.input += input;
  g.output += output;
  g.cache_read += cacheRead;
  g.cache_write += cacheWrite;
  g.prompt_tokens += input + cacheRead + cacheWrite;
  g.cost_usd += cost;
  g.cache_hit_rate = g.prompt_tokens > 0 ? g.cache_read / g.prompt_tokens : null;
}

function stripModelPrefix(model: string): string {
  return model.replace(/^(bifrost\/)?(vertex\/|anthropic\/)?/, "");
}

function parseCsvLine(line: string): string[] {
  const cells: string[] = [];
  let cur = "";
  let inQuotes = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i]!;
    if (ch === '"') {
      if (inQuotes && line[i + 1] === '"') {
        cur += '"';
        i++;
      } else {
        inQuotes = !inQuotes;
      }
    } else if (ch === "," && !inQuotes) {
      cells.push(cur.trim());
      cur = "";
    } else {
      cur += ch;
    }
  }
  cells.push(cur.trim());
  return cells;
}

function comparePrompts(
  prevMsgs: unknown[],
  curMsgs: unknown[],
  scope: "messages" | "system+messages",
): CacheBreakDiff {
  const len = Math.max(prevMsgs.length, curMsgs.length);
  for (let i = 0; i < len; i++) {
    if (i >= prevMsgs.length) {
      // Prev was shorter; check if prev was exact prefix
      const curStr = JSON.stringify(curMsgs[i]);
      return {
        scope,
        prefix_changed: false,
        index: i,
        char: 0,
        tokens_from: 0,
        prev_tokens_to: 0,
        cur_tokens_to: Math.ceil(curStr.length / 4),
      };
    }
    if (i >= curMsgs.length) {
      // Prev has more messages than cur; changed
      const prevStr = JSON.stringify(prevMsgs[i]);
      return {
        scope,
        prefix_changed: true,
        index: i,
        char: 0,
        tokens_from: 0,
        prev_tokens_to: Math.ceil(prevStr.length / 4),
        cur_tokens_to: 0,
      };
    }

    const prevStr = JSON.stringify(prevMsgs[i]);
    const curStr = JSON.stringify(curMsgs[i]);
    if (prevStr !== curStr) {
      let char = 0;
      while (char < prevStr.length && char < curStr.length && prevStr[char] === curStr[char]) {
        char++;
      }
      const isPrefix = char === prevStr.length;
      return {
        scope,
        prefix_changed: !isPrefix,
        index: i,
        char,
        tokens_from: Math.ceil(char / 4),
        prev_tokens_to: Math.ceil(prevStr.length / 4),
        cur_tokens_to: Math.ceil(curStr.length / 4),
      };
    }
  }

  return {
    scope,
    prefix_changed: false,
    index: 0,
    char: 0,
    tokens_from: 0,
    prev_tokens_to: 0,
    cur_tokens_to: 0,
  };
}

function findJsonlFiles(root: string): string[] {
  if (!existsSync(root)) return [];
  const files: string[] = [];
  for (const ent of readdirSync(root, { withFileTypes: true })) {
    const full = join(root, ent.name);
    if (ent.isDirectory()) {
      for (const sub of readdirSync(full, { withFileTypes: true })) {
        if (sub.isFile() && sub.name.endsWith(".jsonl")) {
          files.push(join(full, sub.name));
        }
      }
    } else if (ent.isFile() && ent.name.endsWith(".jsonl")) {
      files.push(full);
    }
  }
  return files;
}

function findDurableSqliteFiles(root: string): string[] {
  if (!existsSync(root)) return [];
  const files: string[] = [];
  for (const plan of readdirSync(root, { withFileTypes: true })) {
    if (!plan.isDirectory()) continue;
    const durableDir = join(root, plan.name, "durable");
    if (existsSync(durableDir)) {
      for (const ent of readdirSync(durableDir, { withFileTypes: true })) {
        if (
          ent.isFile() &&
          ent.name.endsWith(".sqlite") &&
          !ent.name.endsWith("-wal") &&
          !ent.name.endsWith("-shm")
        ) {
          files.push(join(durableDir, ent.name));
        }
      }
    }
  }
  return files;
}

export function report(opts: ModelCostOptions): ModelCostReport {
  const sinceMs = opts.since !== undefined ? Date.parse(opts.since) : null;
  const untilMs = opts.until !== undefined ? Date.parse(opts.until) : null;

  const res: ModelCostReport = {
    window: { since: opts.since ?? null, until: opts.until ?? null },
    files: { pi: 0, durable: 0, bifrost: 0 },
    skipped_lines: 0,
    zero_usage_turns: 0,
    totals: emptySpendGroup(),
    by_role: {
      worker: emptySpendGroup(),
      manager: emptySpendGroup(),
      lead: emptySpendGroup(),
      other: emptySpendGroup(),
    },
    by_model: {},
    by_day: {},
    cache_breaks: [],
    reconciliation: [],
  };

  // Local bifrost usage tracking for reconciliation
  const localBifrostCostByDayModel = new Map<string, number>();
  const localBifrostCacheReadByDay = new Map<string, number>();

  // 1. Parse pi session files
  const piRoot = opts.root ?? join(homedir(), ".pi", "agent", "sessions");
  if (opts.root !== undefined && !existsSync(opts.root)) {
    throw new Error(`cannot read ${opts.root}`);
  }
  const piFiles = findJsonlFiles(piRoot);
  res.files.pi = piFiles.length;

  for (const filePath of piFiles) {
    let content: string;
    try {
      content = readFileSync(filePath, "utf8");
    } catch (e) {
      throw new Error(`cannot read ${filePath}: ${e instanceof Error ? e.message : String(e)}`);
    }

    let sessionName: string | null = null;
    let sessionId: string | null = null;
    const entries: Record<string, unknown>[] = [];

    for (const rawLine of content.split("\n")) {
      const trimmed = rawLine.trim();
      if (trimmed === "") continue;
      try {
        const obj = JSON.parse(trimmed) as Record<string, unknown>;
        if (typeof obj === "object" && obj !== null && !Array.isArray(obj)) {
          entries.push(obj);
          if (obj["type"] === "session" && typeof obj["id"] === "string") {
            sessionId = obj["id"];
          }
          if (obj["type"] === "session_info" && typeof obj["name"] === "string") {
            sessionName = obj["name"];
          }
        }
      } catch {
        res.skipped_lines += 1;
      }
    }

    const role = roleOf(sessionName);
    const sessionKey = sessionName ?? sessionId ?? basename(filePath);

    interface TurnState {
      time: string;
      timestampMs: number;
      model: string;
      cacheRead: number;
      userMessages: unknown[];
    }

    let prevTurn: TurnState | null = null;
    let eventsSinceLastTurn: string[] = [];
    let userMessagesSinceLastTurn: unknown[] = [];

    for (const entry of entries) {
      const type = typeof entry["type"] === "string" ? entry["type"] : "";
      if (
        type === "compaction" ||
        type === "context_edit" ||
        type === "model_change" ||
        type === "thinking_level_change"
      ) {
        eventsSinceLastTurn.push(type);
      }

      const msg = entry["message"];
      if (type === "message" && typeof msg === "object" && msg !== null && !Array.isArray(msg)) {
        const m = msg as Record<string, unknown>;
        const msgRole = typeof m["role"] === "string" ? m["role"] : "";

        if (msgRole === "user") {
          userMessagesSinceLastTurn.push(m);
        } else if (msgRole === "assistant") {
          const rawTs = typeof entry["timestamp"] === "string" ? entry["timestamp"] : (typeof m["timestamp"] === "string" ? m["timestamp"] : "");
          const tsMs = Date.parse(rawTs);

          if (sinceMs !== null && (Number.isNaN(tsMs) || tsMs < sinceMs)) {
            continue;
          }
          if (untilMs !== null && (Number.isNaN(tsMs) || tsMs >= untilMs)) {
            continue;
          }

          const u = m["usage"] as Record<string, unknown> | undefined;
          const input = typeof u?.["input"] === "number" ? u["input"] : 0;
          const output = typeof u?.["output"] === "number" ? u["output"] : 0;
          const cacheRead = typeof u?.["cacheRead"] === "number" ? u["cacheRead"] : 0;
          const cacheWrite = typeof u?.["cacheWrite"] === "number" ? u["cacheWrite"] : 0;
          const costObj = u?.["cost"] as Record<string, unknown> | undefined;
          const cost = typeof costObj?.["total"] === "number" ? costObj["total"] : (typeof u?.["cost"] === "number" ? u["cost"] : 0);

          if (input === 0 && output === 0 && cacheRead === 0 && cacheWrite === 0) {
            res.zero_usage_turns += 1;
            continue;
          }

          const modelName = typeof m["model"] === "string" ? m["model"] : "unknown";
          const day = Intl.DateTimeFormat("en-CA", { timeZone: "America/Toronto" }).format(new Date(tsMs));

          // Aggregate into totals, by_role, by_model, by_day
          addTurnToSpendGroup(res.totals, input, output, cacheRead, cacheWrite, cost);
          addTurnToSpendGroup(res.by_role[role], input, output, cacheRead, cacheWrite, cost);

          if (!(modelName in res.by_model)) {
            res.by_model[modelName] = emptySpendGroup();
          }
          addTurnToSpendGroup(res.by_model[modelName]!, input, output, cacheRead, cacheWrite, cost);

          if (!(day in res.by_day)) {
            res.by_day[day] = emptySpendGroup();
          }
          addTurnToSpendGroup(res.by_day[day]!, input, output, cacheRead, cacheWrite, cost);

          // Check bifrost reconciliation
          const provider = typeof m["provider"] === "string" ? m["provider"] : (modelName.startsWith("bifrost/") ? "bifrost" : "");
          if (provider === "bifrost" || modelName.startsWith("bifrost/")) {
            const utcDay = new Date(tsMs).toISOString().slice(0, 10);
            const stripped = stripModelPrefix(modelName);
            const key = `${utcDay}:${stripped}`;
            localBifrostCostByDayModel.set(key, (localBifrostCostByDayModel.get(key) ?? 0) + cost);
            localBifrostCacheReadByDay.set(utcDay, (localBifrostCacheReadByDay.get(utcDay) ?? 0) + cacheRead);
          }

          // Cache break check
          if (prevTurn !== null && prevTurn.model === modelName) {
            if (prevTurn.cacheRead >= 5000 && cacheRead < 0.5 * prevTurn.cacheRead) {
              const gap_s = Math.round((tsMs - prevTurn.timestampMs) / 1000);
              const idle_gap = gap_s > 300;
              const breakClass: "to_zero" | "partial" = cacheRead === 0 ? "to_zero" : "partial";
              const cause = eventsSinceLastTurn.length > 0 ? eventsSinceLastTurn[0]! : "none_logged";

              const cb: CacheBreak = {
                source: filePath,
                session: sessionKey,
                time: rawTs,
                model: modelName,
                role,
                class: breakClass,
                prev_cache_read: prevTurn.cacheRead,
                cache_read: cacheRead,
                gap_s,
                idle_gap,
                cause,
              };

              if (opts.diffPrefix) {
                cb.diff = comparePrompts(prevTurn.userMessages, userMessagesSinceLastTurn, "messages");
              }

              res.cache_breaks.push(cb);
            }
          }

          prevTurn = {
            time: rawTs,
            timestampMs: tsMs,
            model: modelName,
            cacheRead,
            userMessages: userMessagesSinceLastTurn,
          };
          eventsSinceLastTurn = [];
          userMessagesSinceLastTurn = [];
        }
      }
    }
  }

  // 2. Parse durable worker sqlite sessions
  const durableRoot = opts.durableRoot ?? join(homedir(), ".cpd", "run");
  if (opts.durableRoot !== undefined && !existsSync(opts.durableRoot)) {
    throw new Error(`cannot read ${opts.durableRoot}`);
  }
  const durableFiles = findDurableSqliteFiles(durableRoot);
  res.files.durable = durableFiles.length;

  for (const dbPath of durableFiles) {
    let db: DatabaseSync;
    try {
      db = new DatabaseSync(dbPath, { readOnly: true });
    } catch (e) {
      throw new Error(`cannot read ${dbPath}: ${e instanceof Error ? e.message : String(e)}`);
    }

    try {
      const stmt = db.prepare(
        `SELECT conversation_id, record FROM entries WHERE record LIKE '%"kind":"pi.assistant"%' ORDER BY id ASC`,
      );
      const rows = stmt.all() as { conversation_id: number; record: string }[];

      interface DurableTurnState {
        time: string;
        timestampMs: number;
        model: string;
        cacheRead: number;
        userMessages: unknown[];
      }
      const prevTurnsByConv = new Map<number, DurableTurnState>();

      for (const row of rows) {
        let rec: Record<string, unknown>;
        try {
          rec = JSON.parse(row.record);
        } catch {
          res.skipped_lines += 1;
          continue;
        }

        const convId = typeof rec["conversationId"] === "number" ? rec["conversationId"] : row.conversation_id;
        const models = Array.isArray(rec["model"]) ? rec["model"] : [rec["model"]];

        for (const item of models) {
          if (typeof item !== "object" || item === null) continue;
          const m = item as Record<string, unknown>;
          if (m["role"] !== "assistant") continue;

          const rawTs = m["timestamp"];
          const tsMs = typeof rawTs === "number" ? rawTs : (typeof rawTs === "string" ? Date.parse(rawTs) : NaN);
          const isoTs = Number.isNaN(tsMs) ? "" : new Date(tsMs).toISOString();

          if (sinceMs !== null && (Number.isNaN(tsMs) || tsMs < sinceMs)) {
            continue;
          }
          if (untilMs !== null && (Number.isNaN(tsMs) || tsMs >= untilMs)) {
            continue;
          }

          const u = m["usage"] as Record<string, unknown> | undefined;
          const input = typeof u?.["input"] === "number" ? u["input"] : 0;
          const output = typeof u?.["output"] === "number" ? u["output"] : 0;
          const cacheRead = typeof u?.["cacheRead"] === "number" ? u["cacheRead"] : 0;
          const cacheWrite = typeof u?.["cacheWrite"] === "number" ? u["cacheWrite"] : 0;
          const costObj = u?.["cost"] as Record<string, unknown> | undefined;
          const cost = typeof costObj?.["total"] === "number" ? costObj["total"] : (typeof u?.["cost"] === "number" ? u["cost"] : 0);

          if (input === 0 && output === 0 && cacheRead === 0 && cacheWrite === 0) {
            res.zero_usage_turns += 1;
            continue;
          }

          const provider = typeof m["provider"] === "string" ? m["provider"] : "";
          let modelName = typeof m["model"] === "string" ? m["model"] : "unknown";
          if (provider && !modelName.startsWith(provider + "/")) {
            modelName = `${provider}/${modelName}`;
          }

          const day = Intl.DateTimeFormat("en-CA", { timeZone: "America/Toronto" }).format(new Date(tsMs));

          // Durable sessions are all 'worker'
          addTurnToSpendGroup(res.totals, input, output, cacheRead, cacheWrite, cost);
          addTurnToSpendGroup(res.by_role.worker, input, output, cacheRead, cacheWrite, cost);

          if (!(modelName in res.by_model)) {
            res.by_model[modelName] = emptySpendGroup();
          }
          addTurnToSpendGroup(res.by_model[modelName]!, input, output, cacheRead, cacheWrite, cost);

          if (!(day in res.by_day)) {
            res.by_day[day] = emptySpendGroup();
          }
          addTurnToSpendGroup(res.by_day[day]!, input, output, cacheRead, cacheWrite, cost);

          // Check bifrost reconciliation
          if (provider === "bifrost" || modelName.startsWith("bifrost/")) {
            const utcDay = new Date(tsMs).toISOString().slice(0, 10);
            const stripped = stripModelPrefix(modelName);
            const key = `${utcDay}:${stripped}`;
            localBifrostCostByDayModel.set(key, (localBifrostCostByDayModel.get(key) ?? 0) + cost);
            localBifrostCacheReadByDay.set(utcDay, (localBifrostCacheReadByDay.get(utcDay) ?? 0) + cacheRead);
          }

          // Cache break check
          const prev = prevTurnsByConv.get(convId);
          if (prev && prev.model === modelName) {
            if (prev.cacheRead >= 5000 && cacheRead < 0.5 * prev.cacheRead) {
              const gap_s = Math.round((tsMs - prev.timestampMs) / 1000);
              const idle_gap = gap_s > 300;
              const breakClass: "to_zero" | "partial" = cacheRead === 0 ? "to_zero" : "partial";

              const cb: CacheBreak = {
                source: dbPath,
                session: String(convId),
                time: isoTs,
                model: modelName,
                role: "worker",
                class: breakClass,
                prev_cache_read: prev.cacheRead,
                cache_read: cacheRead,
                gap_s,
                idle_gap,
                cause: null,
              };

              if (opts.diffPrefix) {
                cb.diff = comparePrompts([], [], "system+messages");
              }

              res.cache_breaks.push(cb);
            }
          }

          prevTurnsByConv.set(convId, {
            time: isoTs,
            timestampMs: tsMs,
            model: modelName,
            cacheRead,
            userMessages: [],
          });
        }
      }
    } finally {
      db.close();
    }
  }

  // 3. Parse Bifrost CSV and compute reconciliation
  if (opts.bifrostCsv !== undefined) {
    if (!existsSync(opts.bifrostCsv)) {
      throw new Error(`cannot read ${opts.bifrostCsv}`);
    }
    res.files.bifrost = 1;

    let csvText: string;
    try {
      csvText = readFileSync(opts.bifrostCsv, "utf8").replace(/^\uFEFF/, "");
    } catch (e) {
      throw new Error(`cannot read ${opts.bifrostCsv}: ${e instanceof Error ? e.message : String(e)}`);
    }

    const lines = csvText.split("\n");
    let currentSection = "";
    let costHeader: string[] | null = null;
    const costRows: { day: string; costs: Record<string, number> }[] = [];
    let tokensHeader: string[] | null = null;
    const bifrostCachedReadByDay = new Map<string, number>();

    let hasOverviewCost = false;

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i]!.trim();
      if (line.startsWith("# ")) {
        currentSection = line.slice(2).trim();
        if (currentSection === "overview-cost") {
          hasOverviewCost = true;
        }
        continue;
      }
      if (line === "") continue;

      if (currentSection === "overview-tokens") {
        if (!tokensHeader) {
          tokensHeader = parseCsvLine(line);
        } else {
          const cells = parseCsvLine(line);
          const ts = cells[0] ?? "";
          const day = ts.slice(0, 10);
          const cachedReadIdx = tokensHeader.indexOf("Cached Read Tokens");
          if (cachedReadIdx !== -1 && cells[cachedReadIdx] !== undefined) {
            const val = parseFloat(cells[cachedReadIdx]!);
            if (Number.isNaN(val)) {
              throw new Error(`non-numeric cell in ${opts.bifrostCsv}: ${cells[cachedReadIdx]}`);
            }
            bifrostCachedReadByDay.set(day, val);
          }
        }
      } else if (currentSection === "overview-cost") {
        if (!costHeader) {
          costHeader = parseCsvLine(line);
        } else {
          const cells = parseCsvLine(line);
          const ts = cells[0] ?? "";
          const day = ts.slice(0, 10);
          const costs: Record<string, number> = {};
          for (let c = 1; c < costHeader.length; c++) {
            const colName = costHeader[c]!;
            if (colName === "Total Cost") continue;
            const rawVal = cells[c] ?? "0";
            const val = parseFloat(rawVal);
            if (Number.isNaN(val)) {
              throw new Error(`non-numeric cell in ${opts.bifrostCsv}: ${rawVal}`);
            }
            costs[colName] = val;
          }
          costRows.push({ day, costs });
        }
      }
    }

    if (!hasOverviewCost) {
      throw new Error(`missing # overview-cost in ${opts.bifrostCsv}`);
    }

    for (const r of costRows) {
      for (const [model, bifrostCost] of Object.entries(r.costs)) {
        const localCost = localBifrostCostByDayModel.get(`${r.day}:${model}`) ?? 0;
        const delta = bifrostCost - localCost;
        const ratio = bifrostCost !== 0 ? localCost / bifrostCost : null;
        const localCacheRead = localBifrostCacheReadByDay.get(r.day) ?? 0;
        const bifrostCacheRead = bifrostCachedReadByDay.get(r.day) ?? 0;

        res.reconciliation.push({
          utc_day: r.day,
          model,
          local_cost_usd: localCost,
          bifrost_cost_usd: bifrostCost,
          delta_usd: delta,
          ratio,
          local_cache_read: localCacheRead,
          bifrost_cached_read: bifrostCacheRead,
        });
      }
    }

    res.reconciliation.sort((a, b) => {
      if (a.utc_day !== b.utc_day) return a.utc_day.localeCompare(b.utc_day);
      return a.model.localeCompare(b.model);
    });
  }

  return res;
}

function formatPlainText(r: ModelCostReport): string {
  const lines: string[] = [];

  for (const [name, g] of Object.entries(r.by_role)) {
    const hit = g.cache_hit_rate === null ? "null" : g.cache_hit_rate.toFixed(4);
    lines.push(
      `role ${name} turns=${g.turns} prompt=${g.prompt_tokens} cache_read=${g.cache_read} cache_write=${g.cache_write} output=${g.output} hit_rate=${hit} cost_usd=${g.cost_usd.toFixed(2)}`,
    );
  }

  for (const [name, g] of Object.entries(r.by_model)) {
    const hit = g.cache_hit_rate === null ? "null" : g.cache_hit_rate.toFixed(4);
    lines.push(
      `model ${name} turns=${g.turns} prompt=${g.prompt_tokens} cache_read=${g.cache_read} cache_write=${g.cache_write} output=${g.output} hit_rate=${hit} cost_usd=${g.cost_usd.toFixed(2)}`,
    );
  }

  for (const [name, g] of Object.entries(r.by_day)) {
    const hit = g.cache_hit_rate === null ? "null" : g.cache_hit_rate.toFixed(4);
    lines.push(
      `day ${name} turns=${g.turns} prompt=${g.prompt_tokens} cache_read=${g.cache_read} cache_write=${g.cache_write} output=${g.output} hit_rate=${hit} cost_usd=${g.cost_usd.toFixed(2)}`,
    );
  }

  for (const b of r.cache_breaks) {
    let s = `break ${b.time} ${b.session} ${b.model} class=${b.class} prev=${b.prev_cache_read} cur=${b.cache_read} gap_s=${b.gap_s} idle_gap=${b.idle_gap} cause=${b.cause ?? "none_logged"}`;
    if (b.diff) {
      s += ` prefix_changed=${b.diff.prefix_changed} tokens_from=${b.diff.tokens_from}`;
    }
    lines.push(s);
  }

  for (const row of r.reconciliation) {
    const ratio = row.ratio === null ? "null" : row.ratio.toFixed(4);
    lines.push(
      `recon ${row.utc_day} ${row.model} local=${row.local_cost_usd.toFixed(2)} bifrost=${row.bifrost_cost_usd.toFixed(2)} delta=${row.delta_usd.toFixed(2)} ratio=${ratio}`,
    );
  }

  return lines.join("\n");
}

export async function main(argv: readonly string[]): Promise<number> {
  const opts: ModelCostOptions = {};

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]!;
    if (arg === "--help" || arg === "-h") {
      process.stdout.write(
        "Usage: node pi/measure/model-cost.ts [--json] [--root <dir>] [--durable-root <dir>] [--bifrost-csv <file>] [--since <ISO time>] [--until <ISO time>] [--diff-prefix]\n",
      );
      return 0;
    }
    if (arg === "--json") {
      opts.json = true;
      continue;
    }
    if (arg === "--diff-prefix") {
      opts.diffPrefix = true;
      continue;
    }
    if (
      arg !== "--root" &&
      arg !== "--durable-root" &&
      arg !== "--bifrost-csv" &&
      arg !== "--since" &&
      arg !== "--until"
    ) {
      process.stderr.write(`unknown option: ${arg}\n`);
      return 2;
    }
    const val = argv[++i];
    if (val === undefined) {
      process.stderr.write(`missing value for ${arg}\n`);
      return 2;
    }
    if (arg === "--root") {
      opts.root = val;
    } else if (arg === "--durable-root") {
      opts.durableRoot = val;
    } else if (arg === "--bifrost-csv") {
      opts.bifrostCsv = val;
    } else if (arg === "--since") {
      if (Number.isNaN(Date.parse(val))) {
        process.stderr.write(`--since is not a valid time: ${val}\n`);
        return 2;
      }
      opts.since = val;
    } else if (arg === "--until") {
      if (Number.isNaN(Date.parse(val))) {
        process.stderr.write(`--until is not a valid time: ${val}\n`);
        return 2;
      }
      opts.until = val;
    }
  }

  let rep: ModelCostReport;
  try {
    rep = report(opts);
  } catch (e) {
    process.stderr.write(`${e instanceof Error ? e.message : String(e)}\n`);
    return 2;
  }

  if (opts.json) {
    process.stdout.write(JSON.stringify(rep) + "\n");
  } else {
    process.stdout.write(formatPlainText(rep) + "\n");
  }

  return 0;
}

if (import.meta.main) {
  void main(process.argv.slice(2));
}
