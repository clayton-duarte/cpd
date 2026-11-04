import { CPD_TUI_COMMAND, startPty } from "./pty.ts";
import type { PtyBackend, PtyProcess, PtySinks, PtySpawnOptions } from "./pty.ts";

function fakes(spawnError?: unknown) {
  const spawns: Array<{ file: string; args: string[]; options: PtySpawnOptions }> = [];
  const proc = {
    writes: [] as string[],
    resizes: [] as Array<[number, number]>,
    kills: 0,
    data: undefined as ((d: string) => void) | undefined,
    exit: undefined as ((e: { exitCode: number }) => void) | undefined,
  };
  const process: PtyProcess = {
    onData: (cb) => { proc.data = cb; },
    onExit: (cb) => { proc.exit = cb; },
    write: (d) => { proc.writes.push(d); },
    resize: (c, r) => { proc.resizes.push([c, r]); },
    kill: () => { proc.kills += 1; },
  };
  const backend: PtyBackend = {
    spawn: (file, args, options) => {
      spawns.push({ file, args, options });
      if (spawnError !== undefined) throw spawnError;
      return process;
    },
  };
  const events: string[] = [];
  const sinks: PtySinks = {
    data: (c) => { events.push(`data:${c}`); },
    exit: (c) => { events.push(`exit:${c}`); },
  };
  return { backend, spawns, proc, sinks, events };
}

const size = { cols: 80, rows: 24 };

test("startPty spawns cpd tui --attach with xterm-256color and truecolor", () => {
  const f = fakes();
  startPty({ backend: f.backend, size, cwd: "/home/u", env: { PATH: "/x", TERM: "dumb", COLORTERM: "no" }, sinks: f.sinks });
  expect(CPD_TUI_COMMAND.file).toBe("cpd");
  expect([...CPD_TUI_COMMAND.args]).toEqual(["tui", "--attach"]);
  expect(f.spawns).toHaveLength(1);
  expect(f.spawns[0]).toEqual({
    file: "cpd",
    args: ["tui", "--attach"],
    options: {
      name: "xterm-256color",
      cols: 80,
      rows: 24,
      cwd: "/home/u",
      env: { PATH: "/x", TERM: "xterm-256color", COLORTERM: "truecolor" },
    },
  });
});

test("startPty drops undefined env values", () => {
  const f = fakes();
  startPty({ backend: f.backend, size, cwd: "/h", env: { A: "1", B: undefined }, sinks: f.sinks });
  const env = f.spawns[0]?.options.env ?? {};
  expect("B" in env).toBe(false);
  expect(env.A).toBe("1");
});

test("startPty forwards process data and exit to the sinks", () => {
  const f = fakes();
  startPty({ backend: f.backend, size, cwd: "/h", env: {}, sinks: f.sinks });
  f.proc.data?.("hello");
  f.proc.exit?.({ exitCode: 3 });
  expect(f.events).toEqual(["data:hello", "exit:3"]);
});

test("startPty delegates write and resize to the process", () => {
  const f = fakes();
  const session = startPty({ backend: f.backend, size, cwd: "/h", env: {}, sinks: f.sinks });
  session.write("ls\r");
  session.resize({ cols: 100, rows: 30 });
  expect(f.proc.writes).toEqual(["ls\r"]);
  expect(f.proc.resizes).toEqual([[100, 30]]);
});

test("startPty kills the process once", () => {
  const f = fakes();
  const session = startPty({ backend: f.backend, size, cwd: "/h", env: {}, sinks: f.sinks });
  session.kill();
  session.kill();
  expect(f.proc.kills).toBe(1);
});

test("startPty session methods are no-ops after exit", () => {
  const f = fakes();
  const session = startPty({ backend: f.backend, size, cwd: "/h", env: {}, sinks: f.sinks });
  f.proc.exit?.({ exitCode: 0 });
  expect(() => {
    session.write("x");
    session.resize({ cols: 1, rows: 1 });
    session.kill();
  }).not.toThrow();
  expect(f.proc.writes).toEqual([]);
  expect(f.proc.resizes).toEqual([]);
  expect(f.proc.kills).toBe(0);
  expect(f.events).toEqual(["exit:0"]);
});

test("startPty reports a spawn failure through the sinks without throwing", () => {
  const f = fakes(new Error("boom"));
  const session = startPty({ backend: f.backend, size, cwd: "/h", env: {}, sinks: f.sinks });
  expect(f.events).toEqual(["data:failed to start cpd tui --attach: boom\r\n", "exit:1"]);
  expect(() => {
    session.write("x");
    session.resize({ cols: 1, rows: 1 });
    session.kill();
  }).not.toThrow();
  const g = fakes("plain");
  startPty({ backend: g.backend, size, cwd: "/h", env: {}, sinks: g.sinks });
  expect(g.events).toEqual(["data:failed to start cpd tui --attach: plain\r\n", "exit:1"]);
});
