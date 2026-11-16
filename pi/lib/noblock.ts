/** commands.md §15.11: what a Manager's or a Lead's `bash` call may not wait for. Pure: the adapter (`adapter.ts`) applies it to their `tool_call`. */

import { PREFIXES, simpleCommands } from "./shell.ts";

/** The longest a Manager's or a Lead's bash call may wait, in seconds. */
export const BOUND_SECONDS = 30;

/** The rule's reason, one line naming it (§15.11): the role names who never blocks and where the wait goes instead. */
export const reasonText = (matched: string, role: "manager" | "lead"): string => {
  const tail = role === "manager" ? "is the owning Lead's or Worker's job" : "is left to the engine's Events";
  return `φ: a ${role === "manager" ? "Manager" : "Lead"} never blocks: a wait over ${BOUND_SECONDS} s runs in the background or ${tail}; refused: ${matched}`;
};

const LOOPS = new Set(["while", "until"]);

/** The command word of a simple command and its arguments, after the prefixes; `loop` when a `while` or `until` was among them. */
function strip(words: string[]): { name: string; args: string[]; loop: boolean } | undefined {
  let i = 0;
  let loop = false;
  while (i < words.length && (PREFIXES.has(words[i]!) || /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[i]!))) {
    if (LOOPS.has(words[i]!)) loop = true;
    i++;
  }
  if (i >= words.length) return loop ? { name: "", args: [], loop } : undefined;
  return { name: words[i]!.split("/").pop()!, args: words.slice(i + 1), loop };
}

/** Seconds of one `sleep` or `timeout` duration (`30`, `1.5m`, `2h`, `1d`, `infinity`), or undefined when it is not a literal one. */
function seconds(word: string): number | undefined {
  if (word === "infinity" || word === "inf") return Infinity;
  const m = /^(\d+(?:\.\d*)?|\.\d+)([smhd]?)$/.exec(word);
  if (!m) return undefined;
  return Number(m[1]) * { "": 1, s: 1, m: 60, h: 3600, d: 86400 }[m[2] as "" | "s" | "m" | "h" | "d"];
}

/** Options of `timeout` that take the next word as their value. */
const TIMEOUT_VALUED = new Set(["-s", "--signal", "-k", "--kill-after"]);

const adjacent = (args: string[], a: string, b: string): boolean => args.some((w, i) => w === a && args[i + 1] === b);

/** What of one simple command waits over the bound, as it is shown in the reason, or undefined. */
function waits(name: string, args: string[]): string | undefined {
  switch (name) {
    case "sleep": {
      const durations = args.map(seconds).filter((s): s is number => s !== undefined);
      return durations.reduce((a, b) => a + b, 0) > BOUND_SECONDS ? `sleep ${args.join(" ")}` : undefined;
    }
    case "timeout": {
      let i = 0;
      while (i < args.length && args[i]!.startsWith("-")) i += TIMEOUT_VALUED.has(args[i]!) ? 2 : 1;
      const limit = args[i] === undefined ? undefined : seconds(args[i]!);
      return limit !== undefined && limit > BOUND_SECONDS ? `timeout ${args[i]}` : undefined;
    }
    case "watch":
      return "watch";
    case "tail":
      return args.some((w) => /^-[A-Za-z0-9]*[fF][A-Za-z0-9]*$/.test(w) || /^--follow(=.*)?$/.test(w)) ? "tail -f" : undefined;
    case "cpd":
      return adjacent(args, "events", "follow") ? "cpd events follow" : undefined;
    case "gh":
      if (adjacent(args, "run", "watch")) return "gh run watch";
      return args.includes("checks") && args.some((w) => w === "--watch" || w.startsWith("--watch=")) ? "gh pr checks --watch" : undefined;
    default:
      return undefined;
  }
}

/**
 * The pattern of §15.11 a bash call matches, as the reason shows it, or undefined when it may run: `command` is the bash tool's
 * `command`, `timeout` its own `timeout` input, in seconds. This is a syntactic guard on the tool call, not a child-runtime
 * monitor: `simpleCommands` splits at `&` without retaining foreground/background status, so a long literal `timeout` is
 * refused even when its child command is backgrounded. A bounded background child is not a foreground wait; do not infer its
 * runtime from the Bash tool's wait bound. Other §15.11 patterns (including `sleep`, follow commands, and loop sleeps) remain
 * independently refused wherever they appear in the parsed simple commands.
 */
export function blockingReason(command: string, timeout?: number): string | undefined {
  if (typeof timeout === "number" && timeout > BOUND_SECONDS) return `bash timeout ${timeout}`;
  let inLoop = false;
  for (const words of simpleCommands(command)) {
    const c = strip(words);
    if (!c) continue;
    if (c.loop) inLoop = true;
    if (inLoop && c.name === "sleep") return "while … sleep";
    const hit = waits(c.name, c.args);
    if (hit !== undefined) return hit;
  }
  return undefined;
}
