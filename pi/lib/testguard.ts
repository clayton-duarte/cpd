/** commands.md §15.12: which test runs a Worker's `bash` call may not make. Pure: the adapter (`adapter.ts`) applies it to a Worker's `tool_call`. */

import { PREFIXES, simpleCommands } from "./shell.ts";

/** The rule's reason, one line naming it (§15.12). */
export const reasonText = (matched: string): string =>
  `φ: a Worker runs only the tests its change touches; the full suite is the Gate's, run by the Workflow's open Job; refused: ${matched}`;

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

/** The unfiltered `npm|pnpm test`, `run test` or the Gate `run verify` (pnpm also bare `pnpm verify`), as shown in the reason, or undefined. */
function npmRun(prog: "npm" | "pnpm", args: string[]): string | undefined {
  let i = 0;
  while (i < args.length && args[i]!.startsWith("-")) i++;
  const sub = args[i];
  const isRun = sub === "run" || sub === "run-script";
  const bareVerify = prog === "pnpm" && sub === "verify";
  const script = isRun ? args[i + 1] : bareVerify ? "verify" : sub === "test" || sub === "t" || sub === "tst" ? "test" : undefined;
  if (script === "verify" && (isRun || bareVerify)) return `${prog} run verify`;
  if (script !== "test") return undefined;
  const after = args.slice(i + (isRun ? 2 : 1));
  return after.some((w) => w !== "--" && !w.startsWith("-")) ? undefined : isRun ? `${prog} run test` : `${prog} test`;
}

/**
 * The full unfiltered test run of §15.12 a bash call makes, as the reason shows it, or undefined when it may run: `command` is the
 * bash tool's `command`.
 */
export function unfilteredTestRun(command: string): string | undefined {
  for (const words of simpleCommands(command)) {
    const c = strip(words);
    if (!c) continue;
    const hit = c.name === "cargo" || c.name === "cargo.sh" ? cargoRun(c.name, c.args) : c.name === "npm" || c.name === "pnpm" ? npmRun(c.name, c.args) : undefined;
    if (hit !== undefined) return hit;
  }
  return undefined;
}
