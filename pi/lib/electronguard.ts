/** commands.md §15.28: the electron guard. Pure: applied to every `bash` call of every role. */
import { PREFIXES, simpleCommands } from "./shell.ts";

/** Options of `timeout` that take the next word as their value. */
const TIMEOUT_VALUED = new Set(["-s", "--signal", "-k", "--kill-after"]);
const RUNNERS = new Set(["npx", "bunx", "pnpx"]);
const MANAGERS = new Set(["pnpm", "npm", "yarn", "bun"]);
const EXEC = new Set(["exec", "dlx", "x"]);

/** The words of a simple command after env assignments, prefixes and any `timeout <duration>`. */
function strip(line: string[]): string[] {
  let rest = line;
  for (;;) {
    let i = 0;
    while (i < rest.length && (PREFIXES.has(rest[i]!) || /^[A-Za-z_][A-Za-z0-9_]*=/.test(rest[i]!))) i++;
    rest = rest.slice(i);
    if (rest[0] === undefined || rest[0].split("/").pop() !== "timeout") return rest;
    let j = 1;
    while (j < rest.length && rest[j]!.startsWith("-")) j += TIMEOUT_VALUED.has(rest[j]!) ? 2 : 1;
    rest = rest.slice(j + 1);
  }
}

const isElectron = (w: string | undefined): boolean => w !== undefined && /^electron(@.*)?$/.test(w);

/** The refused command as the reason shows it, or undefined when it may run. */
export function electronBypass(command: string): string | undefined {
  for (const line of simpleCommands(command)) {
    const words = strip(line);
    const head = words[0];
    if (head === undefined) continue;
    if (head === "electron" || head.endsWith("/electron") || head.endsWith("Electron.app/Contents/MacOS/Electron")) return head;
    const name = head.split("/").pop()!;
    if (RUNNERS.has(name)) {
      const arg = words.slice(1).find((w) => !w.startsWith("-"));
      if (isElectron(arg)) return `${head} ${arg}`;
    } else if (MANAGERS.has(name)) {
      for (let i = 1; i + 1 < words.length; i++) {
        if (EXEC.has(words[i]!) && isElectron(words[i + 1])) return `${words.slice(0, i + 2).join(" ")}`;
      }
    }
  }
  return undefined;
}

/** The rule's reason, one line. */
export const reasonText = (matched: string): string =>
  `φ: agents do not run the electron binary; use cpd app (--preview, --reload, --status, --off) (commands.md §15.28); refused: ${matched}`;
