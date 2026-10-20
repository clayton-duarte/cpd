/** commands.md §15.12: which test runs a Worker's `bash` call may not make. Pure: the adapter (`adapter.ts`) applies it to a Worker's `tool_call`. */

/** The rule's reason, one line naming it (§15.12). */
export const reasonText = (matched: string): string =>
  `cpd: a Worker runs only the tests its change touches; the full suite is the Gate's, at pr open; refused: ${matched}`;

/** Words a simple command may start with that are not the command (`FOO=1 sudo timeout 60 x`, `do cargo test`). */
const PREFIXES = new Set(["do", "then", "else", "elif", "if", "!", "{", "}", "time", "nohup", "exec", "command", "sudo", "env", "while", "until"]);

/** The simple commands of a shell line, as words with their quotes taken off. Separators: `;`, `&`, `|`, newline, parentheses, backticks (not inside quotes). */
function simpleCommands(line: string): string[][] {
  const commands: string[][] = [];
  let words: string[] = [];
  let word: string | undefined;
  let quote: string | undefined;
  const endWord = () => {
    if (word !== undefined) words.push(word);
    word = undefined;
  };
  const endCommand = () => {
    endWord();
    if (words.length > 0) commands.push(words);
    words = [];
  };
  for (let i = 0; i < line.length; i++) {
    const c = line[i]!;
    if (quote === "'") {
      if (c === "'") quote = undefined;
      else word += c;
    } else if (quote === '"') {
      if (c === '"') quote = undefined;
      else if (c === "\\" && i + 1 < line.length) word += line[++i];
      else word += c;
    } else if (c === "'" || c === '"') {
      quote = c;
      word ??= "";
    } else if (c === "\\" && i + 1 < line.length) {
      word = (word ?? "") + line[++i];
    } else if (c === "#" && word === undefined) {
      while (i + 1 < line.length && line[i + 1] !== "\n") i++;
    } else if (c === " " || c === "\t") {
      endWord();
    } else if (";&|\n()`".includes(c)) {
      endCommand();
    } else {
      word = (word ?? "") + c;
    }
  }
  endCommand();
  return commands;
}

/** Options of `timeout` that take the next word as their value. */
const TIMEOUT_VALUED = new Set(["-s", "--signal", "-k", "--kill-after"]);

/** The command word of a simple command (its basename) and its arguments, after the prefixes and any `timeout N`. */
function strip(words: string[]): { name: string; args: string[] } | undefined {
  let rest = words;
  for (;;) {
    let i = 0;
    while (i < rest.length && (PREFIXES.has(rest[i]!) || /^[A-Za-z_][A-Za-z0-9_]*=/.test(rest[i]!))) i++;
    if (i >= rest.length) return undefined;
    const name = rest[i]!.split("/").pop()!;
    if (name !== "timeout") return { name, args: rest.slice(i + 1) };
    let j = i + 1;
    while (j < rest.length && rest[j]!.startsWith("-")) j += TIMEOUT_VALUED.has(rest[j]!) ? 2 : 1;
    rest = rest.slice(j + 1); // the duration is skipped too
  }
}

/** Options of `cargo test` and `cargo nextest run` (and of the test binary, after `--`) that take the next word as their value: it is not a test-name filter. */
const VALUED = new Set([
  "-j", "--jobs", "-F", "--features", "--target", "--profile", "--cargo-profile", "--manifest-path", "--target-dir", "--color", "--message-format", "--exclude", "-Z", "--config",
  "--test-threads", "--skip", "--format", "--logfile", "--report-time", "--retries", "--partition", "--run-ignored", "--status-level", "--final-status-level", "--user-config-file", "--build-jobs",
]);

/** Whether the arguments of a cargo test run carry a target: a package, a test, a lib, a bin, a filterset or a positional test-name filter (also after `--`). */
function targeted(args: string[]): boolean {
  for (let i = 0; i < args.length; i++) {
    const w = args[i]!;
    if (w === "--") continue;
    if (/^(--package|--test|--bin|--filterset)(=|$)/.test(w) || w === "--lib") return true;
    if (/^-[pE]/.test(w)) return true; // `-p cpd`, `-pcpd`, `-E expr`
    if (VALUED.has(w)) i++;
    else if (!w.startsWith("-")) return true;
  }
  return false;
}

/** The unfiltered cargo test run of `cargo` / `cargo.sh` with these arguments, as shown in the reason, or undefined. */
function cargoRun(name: string, args: string[]): string | undefined {
  const words = args.filter((w, i) => !(w.startsWith("+") && i === 0)); // a `+toolchain`
  let i = 0;
  while (i < words.length && words[i]!.startsWith("-")) i++;
  let shown: string[];
  if (words[i] === "test") shown = [name, "test"];
  else if (words[i] === "nextest" && words[i + 1] === "run") shown = [name, "nextest", "run"];
  else return undefined;
  const rest = words.slice(i + shown.length - 1);
  if (rest.includes("--workspace")) return [...shown, "--workspace"].join(" ");
  return targeted(rest) ? undefined : shown.join(" ");
}

/** The unfiltered `npm test`, `npm run test` or the Gate `npm run verify`, as shown in the reason, or undefined. */
function npmRun(args: string[]): string | undefined {
  let i = 0;
  while (i < args.length && args[i]!.startsWith("-")) i++;
  const sub = args[i];
  const isRun = sub === "run" || sub === "run-script";
  const script = isRun ? args[i + 1] : sub === "test" || sub === "t" || sub === "tst" ? "test" : undefined;
  if (script === "verify" && isRun) return "npm run verify";
  if (script !== "test") return undefined;
  const after = args.slice(i + (isRun ? 2 : 1));
  return after.some((w) => w !== "--" && !w.startsWith("-")) ? undefined : isRun ? "npm run test" : "npm test";
}

/**
 * The full unfiltered test run of §15.12 a bash call makes, as the reason shows it, or undefined when it may run: `command` is the
 * bash tool's `command`.
 */
export function unfilteredTestRun(command: string): string | undefined {
  for (const words of simpleCommands(command)) {
    const c = strip(words);
    if (!c) continue;
    const hit = c.name === "cargo" || c.name === "cargo.sh" ? cargoRun(c.name, c.args) : c.name === "npm" ? npmRun(c.args) : undefined;
    if (hit !== undefined) return hit;
  }
  return undefined;
}
