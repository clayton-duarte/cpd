import { createPtyTabs, tabCommand } from "./pty-tabs.ts";
import type { PtyTabsDeps } from "./pty-tabs.ts";
import type { PtyProcess, PtySpawnOptions } from "./pty.ts";
import type { SshHost } from "./ssh-config.ts";

const HOSTS: SshHost[] = [
  { alias: "homeserver", hostName: "10.0.0.5" },
  { alias: "buildbox", hostName: "10.0.0.9" },
];

const size = { cols: 80, rows: 24 };

function fakeProc() {
  const proc = {
    writes: [] as string[],
    resizes: [] as Array<[number, number]>,
    kills: 0,
    data: undefined as ((chunk: string) => void) | undefined,
    exit: undefined as ((event: { exitCode: number }) => void) | undefined,
  };
  const process: PtyProcess = {
    onData: (cb) => { proc.data = cb; },
    onExit: (cb) => { proc.exit = cb; },
    write: (data) => { proc.writes.push(data); },
    resize: (cols, rows) => { proc.resizes.push([cols, rows]); },
    kill: () => { proc.kills += 1; },
  };
  return { proc, process };
}

function setup(fail: { spawnError?: unknown } = {}) {
  const spawns: Array<{ file: string; args: string[]; options: PtySpawnOptions }> = [];
  const procs: Array<ReturnType<typeof fakeProc>> = [];
  const events: string[] = [];
  const deps: PtyTabsDeps = {
    spawn: (file, args, options) => {
      spawns.push({ file, args, options });
      if (fail.spawnError !== undefined) throw fail.spawnError;
      const p = fakeProc();
      procs.push(p);
      return p.process;
    },
    commandFor: (tabId) => tabCommand(tabId, HOSTS),
    cwd: "/home/u",
    env: { PATH: "/x" },
    data: (tabId, chunk) => { events.push(`data:${tabId}:${chunk}`); },
    exit: (tabId, code) => { events.push(`exit:${tabId}:${code}`); },
    status: (tabId, status, code) => { events.push(`status:${tabId}:${status}:${code ?? "-"}`); },
  };
  return { spawns, procs, events, tabs: createPtyTabs(deps) };
}

test("start spawns one pty per tab id", () => {
  const t = setup();
  t.tabs.start("local", size);
  t.tabs.start("ssh:homeserver", size);
  expect(t.spawns.map((s) => [s.file, ...s.args])).toEqual([
    ["cpd", "tui", "--attach"],
    ["ssh", "-t", "homeserver", "cpd", "tui", "--attach"],
  ]);
  expect(t.procs).toHaveLength(2);
});

test("a second start for the same tab resizes instead of spawning", () => {
  const t = setup();
  t.tabs.start("local", size);
  t.tabs.start("local", { cols: 100, rows: 30 });
  expect(t.spawns).toHaveLength(1);
  expect(t.procs).toHaveLength(1);
  expect(t.procs[0]?.proc.resizes).toEqual([[100, 30]]);
  expect(t.events).toEqual(["status:local:connecting:-", "status:local:live:-"]);
});

test("start emits connecting then live", () => {
  const t = setup();
  t.tabs.start("ssh:homeserver", size);
  expect(t.events).toEqual([
    "status:ssh:homeserver:connecting:-",
    "status:ssh:homeserver:live:-",
  ]);
});

test("a spawn failure emits failed for that tab", () => {
  const t = setup({ spawnError: new Error("boom") });
  t.tabs.start("ssh:homeserver", size);
  expect(t.events).toEqual([
    "status:ssh:homeserver:connecting:-",
    "status:ssh:homeserver:failed:-",
  ]);
  expect(t.procs).toHaveLength(0);
});

test("an exit with a non-zero code emits failed with the code", () => {
  const t = setup();
  t.tabs.start("ssh:homeserver", size);
  t.procs[0]?.proc.exit?.({ exitCode: 3 });
  expect(t.events).toEqual([
    "status:ssh:homeserver:connecting:-",
    "status:ssh:homeserver:live:-",
    "exit:ssh:homeserver:3",
    "status:ssh:homeserver:failed:3",
  ]);
  t.tabs.start("ssh:homeserver", size);
  expect(t.spawns).toHaveLength(2);
});

test("a remote tab spawns ssh -t alias cpd tui --attach", () => {
  const t = setup();
  t.tabs.start("ssh:homeserver", { cols: 120, rows: 40 });
  expect(t.spawns).toHaveLength(1);
  expect(t.spawns[0]?.file).toBe("ssh");
  expect(t.spawns[0]?.args).toEqual(["-t", "homeserver", "cpd", "tui", "--attach"]);
  expect(t.spawns[0]?.options).toMatchObject({ cols: 120, rows: 40, cwd: "/home/u" });
  expect(tabCommand("ssh:homeserver", HOSTS)).toEqual({
    file: "ssh",
    args: ["-t", "homeserver", "cpd", "tui", "--attach"],
  });
});

test("an unknown tab id spawns nothing", () => {
  const t = setup();
  t.tabs.start("ssh:ghost", size);
  expect(t.spawns).toEqual([]);
  expect(t.events).toEqual([]);
  expect(tabCommand("ssh:ghost", HOSTS)).toBeNull();
});
