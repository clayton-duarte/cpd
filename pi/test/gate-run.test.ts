import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

// The Gate leak check (scripts/gate-run.sh) stops a leftover process before it prints, so it must read the
// name of the test that started it (NEXTEST_TEST_NAME) first and print it in its FAIL message. No real tmux
// and no real leftover of a test: the process list is a fake (a fake /proc for the Linux branch, a fake `ps`
// for the macOS branch). The one real process is a node that waits on its stdin: gate-run stops it.
const SCRIPT = join(import.meta.dirname, "..", "..", "scripts", "gate-run.sh");
const TEST_NAME = "tui::tui_merge::the_confirm_closes_the_view_at_once";

interface Leak {
  /** The `ps` and `/proc` fakes are made in `root`; returns the environment of gate-run and the command it runs. */
  fake: (root: string, pid: number) => { env: Record<string, string>; command: string };
}

function leak({ fake }: Leak): { stderr: string; status: number | null } {
  const root = mkdtempSync(join(tmpdir(), "gate-run-test."));
  const leftover = spawn(process.execPath, ["-e", "process.stdin.resume()"], { stdio: ["pipe", "ignore", "ignore"] });
  try {
    const pid = leftover.pid!;
    const { env, command } = fake(root, pid);
    const run = spawnSync("sh", [SCRIPT, "sh", "-c", command], { encoding: "utf8", env: { ...process.env, TMPDIR: root, MOCK_PID: String(pid), ...env } });
    return { stderr: run.stderr, status: run.status };
  } finally {
    leftover.kill("SIGKILL");
    rmSync(root, { recursive: true, force: true });
  }
}

/** A `ps` that knows one process, `$MOCK_PID`, a child of pid 1 whose environment holds GATE_RUN and `environment`; the rest is the real ps. */
function fakePs(root: string, environment: string): Record<string, string> {
  const bin = join(root, "bin");
  mkdirSync(bin);
  const ps = join(bin, "ps");
  writeFileSync(
    ps,
    `#!/bin/sh
line="zsh -c sleep 120 GATE_RUN=$GATE_RUN ${environment} PATH=/usr/bin"
case "$*" in
  "-axww -E -o pid=,command=") echo "$MOCK_PID $line" ;;
  "-o ppid= -p $MOCK_PID") echo 1 ;;
  "-o ppid= -p 1") echo 0 ;;
  "-o command= -p $MOCK_PID") echo "zsh -c sleep 120" ;;
  "-o command= -p 1") echo "/sbin/launchd" ;;
  "-E -p $MOCK_PID -o command=") echo "$line" ;;
  *) exec /bin/ps "$@" ;;
esac
`,
  );
  chmodSync(ps, 0o755);
  return { PATH: `${bin}:${process.env.PATH}`, GATE_RUN_PROCFS: join(root, "no-proc") };
}

/** A fake /proc with one process, `pid`, a child of pid 1; its environment file is written by the command gate-run runs, which alone knows GATE_RUN. */
function fakeProc(root: string, pid: number, environment: string): { env: Record<string, string>; command: string } {
  const proc = join(root, "proc");
  for (const dir of ["self", "1", String(pid)]) mkdirSync(join(proc, dir), { recursive: true });
  writeFileSync(join(proc, String(pid), "stat"), `${pid} (zsh) S 1 ${pid} ${pid} 0\n`);
  writeFileSync(join(proc, String(pid), "cmdline"), "zsh\0-c\0sleep 120\0");
  writeFileSync(join(proc, "1", "stat"), "1 (init) S 0 1 1 0\n");
  writeFileSync(join(proc, "1", "cmdline"), "/sbin/init\0");
  return {
    env: { GATE_RUN_PROCFS: proc },
    command: `printf 'GATE_RUN=%s\\0${environment}PATH=/usr/bin\\0' "$GATE_RUN" > '${join(proc, String(pid), "environ")}'`,
  };
}

test("a leftover process is reported with the NEXTEST_TEST_NAME of its environment and its parent chain (ps -E, macOS)", () => {
  const { stderr, status } = leak({ fake: (root) => ({ env: fakePs(root, `NEXTEST_TEST_NAME=${TEST_NAME} NEXTEST_BINARY_ID=cpd::tui`), command: "true" }) });
  assert.equal(status, 1, stderr);
  assert.match(stderr, /left 1 things behind/, stderr);
  assert.match(stderr, /process \d+ \(ppid 1\) zsh -c sleep 120 ?\n/, stderr);
  assert.ok(stderr.includes(`    NEXTEST_TEST_NAME=${TEST_NAME}\n`), stderr);
  assert.ok(stderr.includes("    parents: 1 /sbin/launchd\n"), stderr);
});

test("a leftover process is reported with the NEXTEST_TEST_NAME of its environment and its parent chain (/proc, Linux)", () => {
  const { stderr, status } = leak({ fake: (root, pid) => fakeProc(root, pid, `NEXTEST_TEST_NAME=${TEST_NAME}\\0NEXTEST_BINARY_ID=cpd::tui\\0`) });
  assert.equal(status, 1, stderr);
  assert.match(stderr, /left 1 things behind/, stderr);
  assert.match(stderr, /process \d+ \(ppid 1\) zsh -c sleep 120 ?\n/, stderr);
  assert.ok(stderr.includes(`    NEXTEST_TEST_NAME=${TEST_NAME}\n`), stderr);
  assert.match(stderr, /\n    parents: 1 \/sbin\/init ?\n/, stderr);
});

test("a leftover process whose environment has no NEXTEST_TEST_NAME says so instead of naming a test", () => {
  const { stderr, status } = leak({ fake: (root, pid) => fakeProc(root, pid, "") });
  assert.equal(status, 1, stderr);
  assert.match(stderr, /NEXTEST_TEST_NAME=\(not in its environment/, stderr);
});
