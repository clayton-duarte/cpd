import { readFileSync } from "node:fs";

export type CommandUsage = { flags: Record<string, boolean>; positionals: { min: number; max: number | null }; required: string[] };
export type Grammar = Record<string, CommandUsage>;

const FILE = new URL("./cpd-usage.json", import.meta.url);

function parse(text: string): Grammar {
  const bad = (why: string): never => {
    throw new Error(`${FILE.pathname}: ${why}`);
  };
  const o = (v: unknown, what: string): Record<string, unknown> => (typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : bad(`${what} is not an object`));
  const g = o(JSON.parse(text), "the file");
  for (const [k, raw] of Object.entries(g)) {
    const u = o(raw, `"${k}"`);
    for (const [f, takes] of Object.entries(o(u.flags, `"${k}".flags`))) if (typeof takes !== "boolean") bad(`"${k}".flags.${f} is not a boolean`);
    const p = o(u.positionals, `"${k}".positionals`);
    if (!Number.isInteger(p.min)) bad(`"${k}".positionals.min is not an integer`);
    if (!(p.max === null || Number.isInteger(p.max))) bad(`"${k}".positionals.max is not an integer or null`);
    if (!Array.isArray(u.required) || u.required.some((r) => typeof r !== "string")) bad(`"${k}".required is not a list of strings`);
  }
  return g as Grammar;
}

/** The commands of cpd's own grammar (generated from clap, see cli/tests/tooling/usage_grammar.rs). */
export const grammar: Grammar = parse(readFileSync(FILE, "utf8"));

/** What clap would refuse in `argv` (no leading `cpd`), or `undefined`. Sound, not complete: no conflicts, groups or value checks. */
export function usageError(argv: string[], g: Grammar = grammar): string | undefined {
  if (argv.length === 0) return undefined;
  const isPrefix = (p: string) => Object.keys(g).some((k) => k === p || k.startsWith(`${p} `));
  const word = (i: number) => (argv[i] !== undefined && !argv[i]!.startsWith("-") ? argv[i] : undefined);
  let n = 0;
  while (word(n) !== undefined && isPrefix(argv.slice(0, n + 1).join(" "))) n++;
  const path = argv.slice(0, n).join(" ");
  if (n === 0 && word(0) !== undefined) return `unrecognized subcommand '${argv[0]}'`;
  if (n > 0 && !Object.hasOwn(g, path)) {
    return word(n) !== undefined ? `unrecognized subcommand '${argv[n]}'` : `a command is required after '${argv[n - 1]}'`;
  }
  if (!Object.hasOwn(g, path)) return undefined; // the root is not in the grammar: not judged
  const u = g[path]!;
  const seen = new Set<string>();
  let positionals = 0;
  let extra: string | undefined;
  let only = false;
  for (let i = n; i < argv.length; i++) {
    const t = argv[i]!;
    if (!only && t === "--") {
      only = true;
      continue;
    }
    if (!only && t.startsWith("--")) {
      const eq = t.indexOf("=");
      const name = eq < 0 ? t : t.slice(0, eq);
      if (!Object.hasOwn(u.flags, name)) return `unexpected argument '${name}' found`;
      seen.add(name);
      if (u.flags[name]) {
        if (eq < 0) {
          if (i + 1 >= argv.length) return `a value is required for '${name}' but none was supplied`;
          i++;
        }
      } else if (eq >= 0) return `unexpected value '${t.slice(eq + 1)}' for '${name}'`;
      continue;
    }
    if (!only && t.startsWith("-") && t.length > 1) continue;
    positionals++;
    if (u.positionals.max !== null && positionals > u.positionals.max && extra === undefined) extra = t;
  }
  if (extra !== undefined) return `unexpected argument '${extra}' found`;
  if (positionals < u.positionals.min) return `the following required arguments were not provided: positional arguments (${positionals} of ${u.positionals.min})`;
  const missing = u.required.filter((r) => !seen.has(r));
  if (missing.length > 0) return `the following required arguments were not provided: ${missing.join(", ")}`;
  return undefined;
}
