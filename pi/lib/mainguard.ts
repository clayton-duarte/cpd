/** commands.md §15.16: which git commands may not run in a repo's primary checkout. Pure apart from `lstat`: the adapter (`adapter.ts`) applies it to every `bash` call. */
import { lstatSync } from "node:fs";
import { dirname, join, posix } from "node:path";
import { PREFIXES, simpleCommands } from "./shell.ts";

/** The rule's reason, one line naming it and the alternatives (§15.16). */
export const reasonText = (matched: string): string =>
  `cpd: no agent moves HEAD or changes the files of a repo's primary checkout; fetching is allowed, then read origin/main with 'git show origin/main:<path>', 'git log origin/main' or 'git diff origin/main', or in a throwaway 'git worktree add $(mktemp -d) origin/main' that you remove afterwards; refused: ${matched}`;

const REFUSED = new Set(["checkout", "switch", "reset", "rebase", "merge", "pull", "stash"]);

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

/** The words of a simple command after its prefixes (`FOO=1 sudo git ...`). */
const afterPrefixes = (words: string[]): string[] => {
  let i = 0;
  while (i < words.length && (PREFIXES.has(words[i]!) || /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[i]!))) i++;
  return words.slice(i);
};

/** The subcommand of a `git` command and the directory it runs in (undefined when unknown), or undefined when it is not `git`. */
function readGit(args: string[], dir: string | undefined, home: string): { sub: string | undefined; dir: string | undefined } | undefined {
  let at = dir;
  let i = 1;
  for (; i < args.length && args[i]!.startsWith("-"); i++) {
    const w = args[i]!;
    if (w === "-C") at = resolveDir(args[++i] ?? "", at, home);
    else if (VALUED.has(w)) i++;
  }
  return { sub: args[i], dir: at };
}

/**
 * The git command of §15.16 a bash call runs in a primary checkout, as the reason shows it (`git checkout in /p/cpd`), or undefined when
 * it may run: `cwd` is the Session's directory, `home` the home directory.
 */
export function primaryCheckoutMove(command: string, cwd: string, home: string): string | undefined {
  let dir: string | undefined = cwd;
  for (const words of simpleCommands(command)) {
    const args = afterPrefixes(words);
    const name = args[0]?.split("/").pop();
    if (name === "cd") {
      dir = resolveDir(args.slice(1).find((w) => w === "-" || !w.startsWith("-")) ?? "~", dir, home);
    } else if (name === "git" && (args[0] === "git" || args[0]!.endsWith("/git"))) {
      const g = readGit(args, dir, home);
      if (!g?.sub || !REFUSED.has(g.sub) || g.dir === undefined) continue;
      const root = primaryRoot(g.dir);
      if (root !== undefined) return `git ${g.sub} in ${root}`;
    }
  }
  return undefined;
}

/** commands.md §15.16: the stub of `primaryCheckoutWrite`; a later Worker implements the `edit` and `write` guard. */
export function primaryCheckoutWrite(_toolName: string, _input: unknown, _cwd: string, _home: string): string | undefined {
  return undefined;
}
