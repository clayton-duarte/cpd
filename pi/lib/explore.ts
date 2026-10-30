/**
 * The explore guard profile (R11, commands.md §15.22): pattern refusals in the tool_call of a scout or research Worker, keyed on
 * CPD_JOB_KIND. Pure: the caller passes `isInsideRepo` (only `exploreGuard` binds the real one and reads the env). It is NOT an OS sandbox: it refuses patterns, it does not contain a process.
 */

import { existsSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, isAbsolute, resolve } from "node:path";
import { BOUND_SECONDS } from "./noblock.ts";
import { PREFIXES, simpleCommands } from "./shell.ts";

export type ExploreKind = "scout" | "research";

export type Verdict = { block: string } | undefined;

/** The explore kind of a Worker's environment: undefined when CPD_JOB_KIND is unset or empty; any other unknown value fails loud. */
export function exploreKind(env: Record<string, string | undefined>): ExploreKind | undefined {
  const v = env.CPD_JOB_KIND;
  if (v === undefined || v === "") return undefined;
  if (v === "scout" || v === "research") return v;
  throw new Error(`usage: CPD_JOB_KIND is "${v}"; expected "scout" or "research"`);
}

const refuse = (rule: string, what: string): Verdict => ({ block: `φ: explore ${rule}: ${what}` });

const NETWORK = new Set(["curl", "wget", "ssh", "scp", "sftp", "nc", "ftp", "telnet", "dig", "nslookup", "gh", "npm", "pnpm", "yarn", "pip", "brew"]);
const GIT_NETWORK = new Set(["fetch", "pull", "push", "clone", "ls-remote"]);
const GIT_WRITE = new Set(["add", "commit", "checkout", "switch", "reset", "restore", "rebase", "merge", "pull", "push", "fetch", "stash", "clean", "apply", "cherry-pick", "revert", "rm", "mv", "worktree"]);
const CARGO_NETWORK = new Set(["install", "publish", "search"]);
const MUTATORS = new Set(["sed", "rm", "mv", "cp", "mkdir", "touch", "chmod", "ln"]);
const ROOTS = new Set(["/", "/Users", "/home", "~", "$HOME", "${HOME}"]);

type Simple = { name: string; args: string[] };

/** The command word and arguments of a simple command, after prefixes and assignments. */
function strip(words: string[]): Simple | undefined {
  let i = 0;
  while (i < words.length && (PREFIXES.has(words[i]!) || /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[i]!))) i++;
  if (i >= words.length) return undefined;
  return { name: words[i]!.split("/").pop()!, args: words.slice(i + 1) };
}

/** The git subcommand and its arguments, past the global options. */
function gitSub(args: string[]): { sub: string | undefined; rest: string[] } {
  let i = 0;
  while (i < args.length && args[i]!.startsWith("-")) i += args[i] === "-C" || args[i] === "-c" ? 2 : 1;
  return { sub: args[i], rest: args.slice(i + 1) };
}

const norm = (w: string): string => (w.length > 1 ? w.replace(/\/+$/, "") : w);
const absolute = (p: string, cwd: string): string => (isAbsolute(p) ? resolve(p) : resolve(cwd, p));
const operands = (args: string[]): string[] => args.filter((a) => !a.startsWith("-"));

/** Targets of the output redirections of one simple command's words (`> f`, `>>f`, `2>f`), never `&1` or /dev/. */
function redirects(words: string[]): string[] {
  const out: string[] = [];
  for (let i = 0; i < words.length; i++) {
    const m = /^(\d*)>>?(.*)$/.exec(words[i]!);
    if (!m) continue;
    const target = m[2] !== "" ? m[2]! : words[++i];
    if (target !== undefined && !target.startsWith("&")) out.push(target);
  }
  return out;
}

const LITERAL_TOKEN = /(?:^|[^A-Za-z0-9])(?:ghp_|github_pat_|sk-|xox|AKIA)[A-Za-z0-9_-]{6,}/;

/** A literal secret in one command's argv, as shown, or undefined; a value read from a variable or stdin is fine. */
function literalSecret(words: string[]): string | undefined {
  for (let i = 0; i < words.length; i++) {
    const w = words[i]!;
    if (LITERAL_TOKEN.test(w)) return "token";
    const b = /Bearer\s+(\S+)/.exec(w);
    if (b && !b[1]!.startsWith("$")) return "Bearer token";
    if (w === "-u" || w === "--user") {
      const v = words[i + 1];
      if (v !== undefined && v.includes(":") && !v.includes("$")) return "-u user:password";
    }
    const f = /^--(password|token)(?:=(.*))?$/.exec(w);
    if (f) {
      const v = f[2] ?? words[i + 1];
      if (v !== undefined && v !== "" && !v.startsWith("-") && !v.includes("$")) return `--${f[1]} value`;
    }
  }
  return undefined;
}

function bashVerdict(kind: ExploreKind, args: Record<string, unknown>, where: { cwd: string }, isInsideRepo: (p: string) => boolean): Verdict {
  const timeout = args.timeout;
  const command = typeof args.command === "string" ? args.command : "";
  const simples = simpleCommands(command);
  const only = simples.length === 1 ? strip(simples[0]!) : undefined;
  const loneAsk = only?.name === "cpd" && only.args[0] === "ask";
  if (typeof timeout !== "number" || !(timeout > 0) || (!loneAsk && timeout > BOUND_SECONDS)) {
    return refuse("timeout", `every bash call carries a timeout above 0 and at most ${BOUND_SECONDS} s, except a lone cpd ask, which has no upper bound; got ${timeout === undefined ? "none" : String(timeout)}`);
  }
  const inRepo = (p: string): boolean => isInsideRepo(absolute(p, where.cwd));
  for (const words of simples) {
    for (const t of redirects(words)) {
      if (inRepo(t)) return refuse("write", `redirection into a git repository: ${t}`);
    }
    if (kind === "research") {
      const s = literalSecret(words);
      if (s !== undefined) return refuse("secret", `a literal ${s} in a command; read it from stdin or a variable`);
    }
    const c = strip(words);
    if (!c) continue;
    const { name, args: a } = c;
    if (kind === "scout") {
      if (NETWORK.has(name)) return refuse("network", `scout has no network: ${name}`);
      if (name === "git" && GIT_NETWORK.has(gitSub(a).sub ?? "")) return refuse(GIT_WRITE.has(gitSub(a).sub!) ? "network write" : "network", `scout has no network: git ${gitSub(a).sub}`);
      if (name === "cargo" && CARGO_NETWORK.has(operands(a)[0] ?? "")) return refuse("network", `scout has no network: cargo ${operands(a)[0]}`);
    }
    if (name === "git") {
      const sub = gitSub(a).sub ?? "";
      if (GIT_WRITE.has(sub)) return refuse("write", `no git write in explore: git ${sub}`);
    }
    if (name === "tee") {
      for (const t of operands(a)) if (inRepo(t)) return refuse("write", `tee into a git repository: ${t}`);
    }
    if (MUTATORS.has(name)) {
      const ops = operands(a);
      const targets = name === "cp" ? ops.slice(-1) : ops;
      for (const t of targets) if (inRepo(t)) return refuse("write", `${name} into a git repository: ${t}`);
    }
    const roots = (list: string[]) => list.find((w) => ROOTS.has(norm(w)));
    let root: string | undefined;
    if (name === "find") root = roots(a.slice(0, a.findIndex((w) => /^[-!(]/.test(w)) === -1 ? a.length : a.findIndex((w) => /^[-!(]/.test(w))));
    else if (name === "rg") root = roots(operands(a));
    else if (name === "grep" && a.some((w) => w === "--recursive" || /^-[A-Za-z]*[rR][A-Za-z]*$/.test(w))) root = roots(operands(a));
    if (root !== undefined) return refuse("scope", `${name} from ${root} scans the whole machine; give a narrower root`);
  }
  return undefined;
}

/** The refusal of one tool call of an explore Worker of `kind`, or undefined when it may run. `home` is $CPD_HOME. */
export function exploreVerdict(
  kind: ExploreKind,
  call: { name: string; arguments: Record<string, unknown> },
  where: { cwd: string; home: string; plan: string },
  isInsideRepo: (path: string) => boolean,
): Verdict {
  const name = call.name.toLowerCase();
  if (kind === "scout" && (name.startsWith("mcp__") || /web[_-]?(search|fetch)/.test(name))) {
    return refuse("network", `scout has no MCP or web tool: ${call.name}`);
  }
  if (name === "edit" || name === "write") {
    const p = call.arguments.path ?? call.arguments.file_path;
    if (typeof p !== "string") return refuse("write", `${call.name} without a path`);
    const target = absolute(p, where.cwd);
    const explore = resolve(where.home, "run", where.plan, "explore");
    const under = (dir: string): boolean => target === dir || target.startsWith(`${dir}/`);
    if (!under(explore) && !under(resolve(tmpdir()))) return refuse("write", `${call.name} only under ${explore}/ or the temp dir: ${p}`);
    if (!under(explore) && isInsideRepo(target)) return refuse("write", `${call.name} inside a git repository: ${p}`);
    return undefined;
  }
  if (name === "bash") return bashVerdict(kind, call.arguments, where, isInsideRepo);
  return undefined;
}

/** The real `isInsideRepo`: a lexical walk up from the path looking for a `.git` entry; no subprocess. */
export function isInsideRepo(path: string): boolean {
  for (let dir = resolve(path); ; dir = dirname(dir)) {
    if (existsSync(resolve(dir, ".git"))) return true;
    if (dirname(dir) === dir) return false;
  }
}

/** The refusal of one tool call of a Worker started on an explore Job (CPD_JOB_KIND in `env`), or undefined: a Lead, a Manager or a Worker of another kind has no profile. */
export function exploreGuard(role: string | undefined, env: Record<string, string | undefined>, call: { name: string; arguments: Record<string, unknown> }, cwd: string): Verdict {
  const kind = role === "worker" ? exploreKind(env) : undefined;
  if (kind === undefined) return undefined;
  return exploreVerdict(kind, call, { cwd, home: env.CPD_HOME || `${homedir()}/.cpd`, plan: env.CPD_PLAN ?? "" }, isInsideRepo);
}
