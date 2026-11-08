// The per-tab pty registry (ELECTRON.md, `## Remote TUI tabs over ssh`).
//
// createPtyTabs(deps) keeps one pty per tab id behind injected dependencies, so a test uses
// hand-written fakes and never a real pty or ssh:
//   - start(tabId, size) emits `connecting`, then spawns the command of that tab and emits `live`;
//     when that tab already has a session it resizes the live session instead (no second spawn,
//     no status event);
//   - a tab id whose commandFor returns null emits nothing and spawns nothing;
//   - a spawn that throws emits `failed` for that tab with no code;
//   - a process exit emits `exit(tabId, code)` and then `exited` with the code, or `failed` with
//     the code when it is non-zero; the tab's session is dropped either way, so the next start
//     spawns again;
//   - write(tabId, data) and resize(tabId, size) reach only that tab's session, and are no-ops
//     when that tab has no session;
//   - data(tabId, chunk) carries the tab id on every chunk.
//
// Spawn options follow `PtySpawnOptions` (pty.ts): name "xterm-256color", the requested cols and
// rows, `deps.cwd` and `deps.env` without its undefined values, with TERM and COLORTERM set.
//
// Tab ids are `local` and `ssh:<alias>`. tabCommand resolves one against the resolved hosts:
//   - `local` -> CPD_TUI_COMMAND (`cpd tui --attach`);
//   - `ssh:<alias>` -> `{ file: "ssh", args: ["-t", alias, "cpd", "tui", "--attach"] }` when
//     `<alias>` is a host alias in `hosts`;
//   - anything else -> null.

import { tuiAttachCommand } from "../server/cpd-argv.ts";
import type { PtySize, TabStatus } from "./channels.ts";
import { CPD_TUI_COMMAND } from "./pty.ts";
import type { PtyProcess, PtySpawnOptions } from "./pty.ts";
import type { SshHost } from "./ssh-config.ts";

export type { TabStatus };

export interface TabCommand {
  file: string;
  args: readonly string[];
}

export interface PtyTabsDeps {
  spawn(file: string, args: string[], options: PtySpawnOptions): PtyProcess;
  commandFor(tabId: string): TabCommand | null;
  cwd: string;
  env: Record<string, string | undefined>;
  data(tabId: string, chunk: string): void;
  exit(tabId: string, code: number): void;
  status(tabId: string, status: TabStatus, code?: number): void;
}

export interface PtyTabs {
  start(tabId: string, size: PtySize): void;
  write(tabId: string, data: string): void;
  resize(tabId: string, size: PtySize): void;
  dispose(): void;
}

const LOCAL_TAB_ID = "local";
const SSH_PREFIX = "ssh:";

export function tabCommand(tabId: string, hosts: SshHost[]): TabCommand | null {
  if (tabId === LOCAL_TAB_ID) return CPD_TUI_COMMAND;
  if (tabId.startsWith(SSH_PREFIX)) {
    const alias = tabId.slice(SSH_PREFIX.length);
    if (alias !== "" && hosts.some((host) => host.alias === alias)) {
      return tuiAttachCommand(alias);
    }
  }
  return null;
}

function spawnEnv(env: Record<string, string | undefined>): Record<string, string> {
  const resolved: Record<string, string> = {};
  for (const [key, value] of Object.entries(env)) {
    if (value !== undefined) resolved[key] = value;
  }
  resolved["TERM"] = "xterm-256color";
  resolved["COLORTERM"] = "truecolor";
  return resolved;
}

interface LiveSession {
  proc: PtyProcess;
  alive: boolean;
}

export function createPtyTabs(deps: PtyTabsDeps): PtyTabs {
  const sessions = new Map<string, LiveSession>();
  const env = spawnEnv(deps.env);

  const start = (tabId: string, size: PtySize): void => {
    const command = deps.commandFor(tabId);
    if (command === null) return;
    const existing = sessions.get(tabId);
    if (existing && existing.alive) {
      existing.proc.resize(size.cols, size.rows);
      return;
    }
    deps.status(tabId, "connecting");
    let proc: PtyProcess;
    try {
      proc = deps.spawn(command.file, [...command.args], {
        name: "xterm-256color",
        cols: size.cols,
        rows: size.rows,
        cwd: deps.cwd,
        env,
      });
    } catch {
      deps.status(tabId, "failed");
      return;
    }
    const session: LiveSession = { proc, alive: true };
    sessions.set(tabId, session);
    proc.onData((chunk) => {
      deps.data(tabId, chunk);
    });
    proc.onExit((event) => {
      if (!session.alive) return;
      session.alive = false;
      sessions.delete(tabId);
      deps.exit(tabId, event.exitCode);
      if (event.exitCode === 0) deps.status(tabId, "exited", 0);
      else deps.status(tabId, "failed", event.exitCode);
    });
    deps.status(tabId, "live");
  };

  const write = (tabId: string, data: string): void => {
    const session = sessions.get(tabId);
    if (session?.alive) session.proc.write(data);
  };

  const resize = (tabId: string, size: PtySize): void => {
    const session = sessions.get(tabId);
    if (session?.alive) session.proc.resize(size.cols, size.rows);
  };

  const dispose = (): void => {
    for (const session of sessions.values()) {
      if (!session.alive) continue;
      session.alive = false;
      session.proc.kill();
    }
    sessions.clear();
  };

  return { start, write, resize, dispose };
}
