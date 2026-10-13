import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, realpathSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

/** Integration tests may only touch this herdr session, and write only under /tmp/cpd-build/pi-adapter/. */
export const SESSION = "cpd-test";
export const MODEL = "github-copilot/claude-haiku-4.5:low";
export const EXTENSION = fileURLToPath(new URL("../../pi/extension.ts", import.meta.url));
const ROOT = "/tmp/cpd-build/pi-adapter";

export function scratch(prefix: string): string {
  mkdirSync(ROOT, { recursive: true });
  return realpathSync(mkdtempSync(join(ROOT, prefix)));
}

/** `herdr --session cpd-test <args>`; returns the parsed `result`, throws on a herdr error. */
export function herdr(...args: string[]): any {
  const r = spawnSync("herdr", ["--session", SESSION, ...args], { encoding: "utf8" });
  const text = r.stdout.trim() !== "" ? r.stdout : r.stderr;
  if (r.status === 0 && text.trim() === "") return null; // e.g. `pane send-text`
  let env: any;
  try {
    env = JSON.parse(text);
  } catch {
    throw new Error(`herdr ${args.join(" ")} exited ${r.status}: ${text.slice(-500)}`);
  }
  if (env.error) throw new Error(`herdr ${args.join(" ")}: ${env.error.code}: ${env.error.message}`);
  return env.result;
}

export async function waitFor<T>(what: string, fn: () => T | undefined | null | false, timeoutMs: number, everyMs = 250): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const v = fn();
    if (v) return v;
    if (Date.now() > deadline) throw new Error(`timed out after ${timeoutMs} ms waiting for ${what}`);
    await sleep(everyMs);
  }
}

export function readJsonl(path: string): any[] {
  try {
    return readFileSync(path, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return [];
    throw e;
  }
}

/** A Lead pi in a fresh herdr workspace. `env` becomes the workspace environment (pi inherits it). */
export async function startLead(o: { name: string; cwd: string; sessionDir: string; env: Record<string, string>; piArgs: string[]; pathPrefix?: string }) {
  const ws = herdr("workspace", "create", "--cwd", o.cwd, "--label", o.name, "--no-focus", ...Object.entries(o.env).flatMap(([k, v]) => ["--env", `${k}=${v}`]));
  const pane: string = ws.root_pane.pane_id;
  try {
    // The pane's shell rc may reorder PATH, so put the fake bin dir first from inside the shell.
    if (o.pathPrefix !== undefined) {
      herdr("pane", "run", pane, `export PATH='${o.pathPrefix}':"$PATH"`);
      await waitFor("the PATH export", () => screen(pane).includes("export PATH="), 10_000);
    }
    const started = herdr("agent", "start", o.name, "--kind", "pi", "--pane", pane, "--timeout", "180000", "--", "--model", MODEL, "--session-dir", o.sessionDir, ...o.piArgs);
    // With `-e npm:…` pi installs the package first, so `agent start` can return before the session is known.
    const sessionPath: string = await waitFor("the agent session", () => (started.agent.agent_session ?? herdr("agent", "get", o.name).agent.agent_session)?.value, 180_000, 1_000);
    const sessionId = /_([^_/]+)\.jsonl$/.exec(sessionPath)?.[1];
    if (!sessionId) throw new Error(`cannot read the session id from ${sessionPath}`);
    return { workspace: ws.workspace.workspace_id as string, pane, name: o.name, sessionPath, sessionId, argv: started.argv as string[] };
  } catch (e) {
    herdr("workspace", "close", ws.workspace.workspace_id);
    throw e;
  }
}
export type Lead = Awaited<ReturnType<typeof startLead>>;
export const agentStatus = (name: string): string => herdr("agent", "get", name).agent.agent_status;
/** `pane read` prints the screen as plain text, not a JSON envelope. */
export const screen = (pane: string): string => {
  const r = spawnSync("herdr", ["--session", SESSION, "pane", "read", pane, "--source", "visible"], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`herdr pane read ${pane} exited ${r.status}: ${r.stderr}`);
  return r.stdout;
};
