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

import type { PtySize } from "./channels.ts";
import type { PtyProcess, PtySpawnOptions } from "./pty.ts";
import type { SshHost } from "./ssh-config.ts";

export type TabStatus = "connecting" | "live" | "exited" | "failed";

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

export function createPtyTabs(_deps: PtyTabsDeps): PtyTabs {
  throw new Error("not implemented");
}

export function tabCommand(_tabId: string, _hosts: SshHost[]): TabCommand | null {
  throw new Error("not implemented");
}
