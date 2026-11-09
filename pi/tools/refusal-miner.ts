/**
 * The refusal miner's CLI (contract §5.7; pi/tools/refusal-miner/README.md is the worked contract).
 *
 * Reads every session directory under `~/.pi/agent/sessions/`, mines the guard refusals whose
 * top-level timestamp is at or after `--since`, and prints the ranking tables: by guard / error
 * code, by role (Manager, Lead, Worker, scout) and by model, each row quoting its matched string.
 * Run it with `pnpm run refusals -- --since <ISO timestamp>`.
 */

import { readdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { mineRefusals, type SessionFile } from "../lib/refusal-miner.ts";

function usage(message: string): never {
  process.stderr.write(`refusal-miner: ${message}\nusage: refusal-miner --since <ISO timestamp>\n`);
  process.exit(2);
}

/** The `--since` cutoff, required; an unknown argument is usage. */
function sinceOf(argv: string[]): string {
  let since: string | undefined;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]!;
    if (arg === "--since") {
      since = argv[++i];
      if (since === undefined) usage("--since needs a timestamp");
    } else if (arg.startsWith("--since=")) {
      since = arg.slice("--since=".length);
    } else {
      usage(`unexpected argument '${arg}'`);
    }
  }
  if (since === undefined) usage("--since is required");
  if (Number.isNaN(Date.parse(since))) usage(`'${since}' is not an ISO timestamp`);
  return since;
}

/** Every `*.jsonl` file of every directory under the sessions root, as a SessionFile. */
function readSessions(root: string): SessionFile[] {
  const files: SessionFile[] = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const dir = join(root, entry.name);
    for (const name of readdirSync(dir)) {
      if (!name.endsWith(".jsonl")) continue;
      files.push({ name: `${entry.name}/${name}`, text: readFileSync(join(dir, name), "utf8") });
    }
  }
  return files;
}

const since = sinceOf(process.argv.slice(2));
const root = join(homedir(), ".pi/agent/sessions");
let files: SessionFile[];
try {
  files = readSessions(root);
} catch (e) {
  process.stderr.write(`refusal-miner: cannot read ${root}: ${e instanceof Error ? e.message : String(e)}\n`);
  process.exit(1);
}

const tables = mineRefusals(files, { since });
const pad = (s: string, n: number): string => (s.length >= n ? s : s + " ".repeat(n - s.length));

console.log(`Refusals since ${since} — files ${files.length}, total ${tables.total}`);
console.log("");
console.log("By guard / code");
for (const r of tables.byGuard) console.log(`  ${String(r.count).padStart(6)}  ${pad(r.guard, 40)}  ${r.sample}`);
console.log("");
console.log("By role");
for (const r of tables.byRole) console.log(`  ${String(r.count).padStart(6)}  ${pad(r.guard, 40)}  ${r.role}`);
console.log("");
console.log("By model");
for (const r of tables.byModel) console.log(`  ${String(r.count).padStart(6)}  ${pad(r.guard, 40)}  ${r.model}`);
