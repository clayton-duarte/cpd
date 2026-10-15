import { failure } from "../lib/errors.ts";
import { run } from "../lib/exec.ts";

/** A tmux pane id such as `%7`. */
const PANE_ID = /^%\d+$/;

/**
 * `tmux -S <socket> new-window -d -P -F '#{pane_id}' -n <name> -c <cwd> <command>`: a new detached window on the tmux server
 * at `socket`, running `command` (tmux runs it through the shell) in `cwd`. Returns the id of the window's pane.
 * The window opens in the server's current session: `new-window` cannot take a pane as its target.
 */
export async function newWindow(socket: string, o: { name: string; cwd: string; command: string }): Promise<string> {
  const argv = ["-S", socket, "new-window", "-d", "-P", "-F", "#{pane_id}", "-n", o.name, "-c", o.cwd, o.command];
  const r = await run("tmux", argv, { cwd: process.cwd(), env: process.env, timeoutMs: 30_000 });
  if (r.code !== 0) throw failure("tmux_failed", `tmux new-window exited ${r.code ?? r.signal}: ${r.stderr.trim()}`, { args: argv, code: r.code, stderr: r.stderr.slice(-2000) });
  const pane = r.stdout.trim();
  if (!PANE_ID.test(pane)) throw failure("bad_tool_output", `tmux new-window printed ${JSON.stringify(pane)}, not a pane id`, { args: argv, stdout: r.stdout.slice(0, 500) });
  return pane;
}

/** `tmux -S <socket> <args>`; a non-zero exit is `tmux_failed`. */
async function tmux(socket: string, args: string[]): Promise<string> {
  const argv = ["-S", socket, ...args];
  const r = await run("tmux", argv, { cwd: process.cwd(), env: process.env, timeoutMs: 30_000 });
  if (r.code !== 0) throw failure("tmux_failed", `tmux ${args[0]} exited ${r.code ?? r.signal}: ${r.stderr.trim()}`, { args: argv, code: r.code, stderr: r.stderr.slice(-2000) });
  return r.stdout;
}

/** `'…'`, safe for the shell tmux runs a window's command through. */
export const shellQuote = (s: string): string => `'${s.replaceAll("'", `'\\''`)}'`;

/** `tmux set-option -w -t <pane> <option> <value>`: a window option of the window holding `pane`. */
export async function setWindowOption(socket: string, pane: string, option: string, value: string): Promise<void> {
  await tmux(socket, ["set-option", "-w", "-t", pane, option, value]);
}

/** `tmux send-keys -t <pane> [-l] <keys…>`: `literal` types the text as is, otherwise each key is a tmux key name (`Enter`). */
export async function sendKeys(socket: string, pane: string, keys: string[], literal = false): Promise<void> {
  await tmux(socket, ["send-keys", "-t", pane, ...(literal ? ["-l"] : []), ...keys]);
}

/** `tmux kill-window -t <pane>`: the window holding `pane`. */
export async function killWindow(socket: string, pane: string): Promise<void> {
  await tmux(socket, ["kill-window", "-t", pane]);
}
