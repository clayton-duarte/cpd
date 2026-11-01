/** commands.md §15.16: which git commands, bash write forms and `edit`/`write` calls may not touch a repo's primary checkout. Pure apart from `lstat`: the adapter (`adapter.ts`) applies it to every `bash`, `edit` and `write` call. */
import { lstatSync } from "node:fs";
import { dirname, join, posix } from "node:path";
import { PREFIXES, simpleCommands } from "./shell.ts";

/** The rule's reason, one line naming it and the alternatives (§15.16). */
export const reasonText = (matched: string): string =>
  `φ: no agent moves HEAD or changes the files of a repo's primary checkout; fetching is allowed, then read origin/main with 'git show origin/main:<path>', 'git log origin/main' or 'git diff origin/main', or in a throwaway 'git worktree add $(mktemp -d) origin/main' that you remove afterwards; to change files, work on a branch in a worktree ('git worktree add -b <branch> <dir> origin/main', or the worktree of your Job) and let a Workflow's pull request carry the change; refused: ${matched}`;

const REFUSED = new Set([
  "checkout",
  "switch",
  "reset",
  "rebase",
  "merge",
  "pull",
  "stash",
  "restore",
  "clean",
  "commit",
  "cherry-pick",
  "revert",
  "am",
  "apply",
  "rm",
  "mv",
  "add",
]);

/** Global options of `git` that take the next word as their value. */
const VALUED = new Set(["-c", "--git-dir", "--work-tree", "--namespace", "--super-prefix", "--config-env"]);

/** The directory a word names, or undefined when it cannot be resolved; `dir` is the current directory (undefined when unknown). */
function resolveDir(word: string, dir: string | undefined, home: string): string | undefined {
  const m = /^(~|\$HOME|\$\{HOME\})(\/.*)?$/.exec(word);
  const w = m ? home + (m[2] ?? "") : word;
  if (w === "-" || /^~|[$`*?[{]/.test(w)) return undefined;
  if (w.startsWith("/")) return posix.normalize(w);
  return dir === undefined ? undefined : posix.resolve(dir, w);
}

/** A write target: resolves like `resolveDir`, but a glob is judged by its literal leading directory (a `$`, a backtick, `~user` and `-` cannot be resolved). */
function resolveTarget(word: string, dir: string | undefined, home: string): string | undefined {
  const m = /^(~|\$HOME|\$\{HOME\})(\/.*)?$/.exec(word);
  const w = m ? home + (m[2] ?? "") : word;
  if (w === "-" || /^~[^/]/.test(w) || /[$`]/.test(w)) return undefined;
  let candidate = w;
  if (/[*?[{]/.test(w)) {
    const segments = w.split("/");
    const first = segments.findIndex((s) => /[*?[{]/.test(s));
    const literal = segments.slice(0, first).join("/");
    candidate = literal === "" ? (w.startsWith("/") ? "/" : ".") : literal;
  }
  if (candidate.startsWith("/")) return posix.normalize(candidate);
  return dir === undefined ? undefined : posix.resolve(dir, candidate);
}

/** The root of the primary checkout `dir` is in: the nearest `.git` at or above it is a directory. Undefined for a linked worktree, no repository or a missing `dir`. */
function primaryRoot(dir: string): string | undefined {
  try {
    lstatSync(dir);
  } catch {
    return undefined;
  }
  for (let d = dir; ; d = dirname(d)) {
    try {
      const s = lstatSync(join(d, ".git"));
      return s.isDirectory() ? d : undefined;
    } catch {
      /* no `.git` here: go up */
    }
    if (dirname(d) === d) return undefined;
  }
}

/** The root of the primary checkout a write target is in: that of its nearest existing ancestor, because a write creates the missing directories. */
function primaryRootOfPath(path: string): string | undefined {
  for (let p = path; ; p = dirname(p)) {
    try {
      lstatSync(p);
      return primaryRoot(p);
    } catch {
      /* not there yet: try its parent */
    }
    if (dirname(p) === p) return undefined;
  }
}

/** A target that is in a primary checkout: its absolute path and that root. */
function judgeTarget(word: string, dir: string | undefined, home: string): { path: string; root: string } | undefined {
  const path = resolveTarget(word, dir, home);
  if (path === undefined) return undefined;
  const root = primaryRootOfPath(path);
  return root === undefined ? undefined : { path, root };
}

/** The words of a simple command after its prefixes (`FOO=1 sudo git ...`). */
const afterPrefixes = (words: string[]): string[] => {
  let i = 0;
  while (i < words.length && (PREFIXES.has(words[i]!) || /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[i]!))) i++;
  return words.slice(i);
};

/** The subcommand of a `git` command and the directory it runs in (undefined when unknown), or undefined when it is not `git`. */
function readGit(args: string[], dir: string | undefined, home: string): { sub: string | undefined; dir: string | undefined } | undefined {
  if (args[0] !== "git" && !args[0]?.endsWith("/git")) return undefined;
  let at = dir;
  let i = 1;
  for (; i < args.length && args[i]!.startsWith("-"); i++) {
    const w = args[i]!;
    if (w === "-C") at = resolveDir(args[++i] ?? "", at, home);
    else if (VALUED.has(w)) i++;
  }
  return { sub: args[i], dir: at };
}

const WRITE_OPERANDS = new Set(["tee", "rm", "touch", "truncate", "mv"]);
const WRITE_DESTINATION = new Set(["cp", "install", "ln"]);
const WRITE_IN_PLACE = new Set(["sed", "perl"]);

/** Whether `sed`/`perl` was given an in-place flag (`-i`, `-i.bak`, `--in-place`, `-ni`, `-pi`). */
const hasInPlace = (args: string[]): boolean =>
  args.some((w) => w === "-i" || w.startsWith("-i") || w === "--in-place" || w.startsWith("--in-place=") || (/^-[A-Za-z]+$/.test(w) && w.endsWith("i")));

/** The first operand (a word not starting with `-`) that is in a primary checkout. */
function operandTarget(args: string[], dir: string | undefined, home: string) {
  for (const w of args.slice(1)) {
    if (w.startsWith("-")) continue;
    const t = judgeTarget(w, dir, home);
    if (t !== undefined) return t;
  }
  return undefined;
}

/** The write command word of a simple command, as `<form> <path> in <root>`, or undefined. */
function writeCommand(args: string[], dir: string | undefined, home: string): string | undefined {
  const word = args[0];
  if (word === undefined) return undefined;
  const name = word.split("/").pop()!;
  if (WRITE_OPERANDS.has(name)) {
    const t = operandTarget(args, dir, home);
    return t === undefined ? undefined : `${name} ${t.path} in ${t.root}`;
  }
  if (WRITE_DESTINATION.has(name)) {
    let targetDir: string | undefined;
    const operands: string[] = [];
    for (let i = 1; i < args.length; i++) {
      const w = args[i]!;
      if (w === "-t") targetDir = args[++i];
      else if (w.startsWith("--target-directory=")) targetDir = w.slice("--target-directory=".length);
      else if (!w.startsWith("-")) operands.push(w);
    }
    const target = targetDir ?? operands[operands.length - 1];
    const t = target === undefined ? undefined : judgeTarget(target, dir, home);
    return t === undefined ? undefined : `${name} ${t.path} in ${t.root}`;
  }
  if (WRITE_IN_PLACE.has(name) && hasInPlace(args)) {
    const t = operandTarget(args, dir, home);
    return t === undefined ? undefined : `${name} ${t.path} in ${t.root}`;
  }
  if (name === "dd") {
    for (const w of args.slice(1)) {
      if (!w.startsWith("of=")) continue;
      const t = judgeTarget(w.slice("of=".length), dir, home);
      if (t !== undefined) return `dd ${t.path} in ${t.root}`;
    }
  }
  if (name === "patch") {
    let at = dir;
    for (let i = 1; i < args.length; i++) {
      const w = args[i]!;
      if (w === "-d") at = resolveTarget(args[++i] ?? "", dir, home);
      else if (w.startsWith("--directory=")) at = resolveTarget(w.slice("--directory=".length), dir, home);
    }
    const t = at === undefined ? undefined : judgeTarget(at, dir, home);
    return t === undefined ? undefined : `patch ${t.path} in ${t.root}`;
  }
  return undefined;
}

/** `>|` holds the tokenizer's separator `|`: keep it in the word as a marker, outside quotes, so the redirect reader sees it. */
function protectClobber(command: string): string {
  let out = "";
  let quote: string | undefined;
  for (let i = 0; i < command.length; i++) {
    const c = command[i]!;
    if (quote !== undefined) {
      out += c;
      if (c === "\\" && quote === '"' && i + 1 < command.length) out += command[++i];
      else if (c === quote) quote = undefined;
    } else if (c === "'" || c === '"') {
      quote = c;
      out += c;
    } else if (c === "\\" && i + 1 < command.length) {
      out += c + command[++i];
    } else if (c === ">" && command[i + 1] === "|") {
      out += ">\u0001";
      i++;
    } else {
      out += c;
    }
  }
  return out;
}

/** A redirect (`>`, `>>`, `>|`, with optional fd digits) whose target is in a primary checkout, as `<form> <path> in <root>`. */
function writeRedirect(args: string[], dir: string | undefined, home: string): string | undefined {
  for (let i = 0; i < args.length; i++) {
    const m = /^(\d*)(>>|>)(\u0001?)(.*)$/.exec(args[i]!);
    if (m === null) continue;
    const form = m[3] === "\u0001" ? ">|" : m[2]!;
    let target = m[4]!;
    if (target === "") target = args[++i] ?? "";
    if (target === "") continue;
    const t = judgeTarget(target, dir, home);
    if (t !== undefined) return `${form} ${t.path} in ${t.root}`;
  }
  return undefined;
}

/**
 * The git command or bash write form of §15.16 a call runs in a primary checkout, as the reason shows it (`git checkout in /p/cpd`,
 * `> /p/cpd/f in /p/cpd`), or undefined when it may run: `cwd` is the Session's directory, `home` the home directory.
 */
export function primaryCheckoutMove(command: string, cwd: string, home: string): string | undefined {
  let dir: string | undefined = cwd;
  for (const words of simpleCommands(protectClobber(command))) {
    const args = afterPrefixes(words);
    const name = args[0]?.split("/").pop();
    if (name === "cd") {
      dir = resolveDir(args.slice(1).find((w) => w === "-" || !w.startsWith("-")) ?? "~", dir, home);
      continue;
    }
    const g = readGit(args, dir, home);
    if (g?.sub && REFUSED.has(g.sub) && g.dir !== undefined) {
      const root = primaryRoot(g.dir);
      if (root !== undefined) return `git ${g.sub} in ${root}`;
    }
    const written = writeCommand(args, dir, home) ?? writeRedirect(args, dir, home);
    if (written !== undefined) return written;
  }
  return undefined;
}

/** commands.md §15.16: the `edit` and `write` half — a call whose path is in a primary checkout, as `<tool> <path> in <root>`. */
export function primaryCheckoutWrite(toolName: string, input: unknown, cwd: string, home: string): string | undefined {
  if (toolName !== "edit" && toolName !== "write") return undefined;
  const raw = (input as { path?: unknown } | null | undefined)?.path;
  if (typeof raw !== "string" || raw === "") return undefined;
  const word = raw.startsWith("@") ? raw.slice(1) : raw;
  const t = judgeTarget(word, cwd, home);
  return t === undefined ? undefined : `${toolName} ${t.path} in ${t.root}`;
}
