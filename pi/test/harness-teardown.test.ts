import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:net";
import { join } from "node:path";
import { test } from "node:test";

// The teardown of the Rust harness (TEARDOWN_SH in cli/tests/common/mod.rs) stops every process of the env's
// tmux panes, even when tmux cannot say which they are. No real tmux: `tmux` is a fake that fails `list-panes`,
// and its socket is a plain listening socket. The panes are node processes that block and carry the
// env's CPD_HOME as the panes of its tmux server do (a process outside the env's group, which only that tag finds).
const ROOT = join(import.meta.dirname, "..", "..");
const TEARDOWN = (() => {
  const source = readFileSync(join(ROOT, "cli", "tests", "common", "mod.rs"), "utf8");
  const start = source.indexOf('const TEARDOWN_SH: &str = r#"') + 'const TEARDOWN_SH: &str = r#"'.length;
  return source.slice(start, source.indexOf('"#;', start));
})();

interface Env {
  dir: string;
  home: string;
  /** The calls of the fake tmux (outside the env's directory, which the teardown removes). */
  calls: string;
  /** Runs the teardown of the env, with pgid of a group that is gone. */
  teardown: () => void;
  /** Starts a detached process (its own session, not a child of this one) that waits, with `CPD_HOME`; returns its pid. */
  pane: (cpdHome: string) => number;
  alive: (pid: number) => boolean;
  stop: () => void;
}

/** An env directory with the fake tmux, `withTmux`: whether a tmux server ever ran in it (its socket is there). */
async function env(withTmux: boolean): Promise<Env> {
  const dir = mkdtempSync("/tmp/td.");
  const calls = `${dir}.calls`;
  const uid = process.getuid!();
  const tmuxdir = join(dir, "tmux");
  const socket = "cpdt-1";
  const home = join(dir, "cpd-home");
  mkdirSync(join(dir, "bin"));
  mkdirSync(join(tmuxdir, `tmux-${uid}`), { recursive: true });
  // The fake tmux: `list-panes` fails (a server too busy, or gone), anything else succeeds and is recorded.
  const tmux = join(dir, "bin", "tmux");
  writeFileSync(tmux, `#!/bin/sh\necho "$*" >> '${calls}'\ncase "$*" in *list-panes*) exit 1 ;; esac\nexit 0\n`);
  chmodSync(tmux, 0o755);
  let server: Server | undefined;
  if (withTmux) {
    server = createServer();
    const listening = new Promise((resolve) => server!.once("listening", resolve));
    server.listen(join(tmuxdir, `tmux-${uid}`, socket));
    await listening;
  } else {
    rmSync(join(tmuxdir, `tmux-${uid}`), { recursive: true });
  }
  const script = join(dir, "teardown.sh");
  const quote = (s: string) => `'${s.replace(/'/g, "'\\''")}'`;
  writeFileSync(script, `dir=${quote(dir)}\nsocket=${quote(socket)}\ntmuxdir=${quote(tmuxdir)}\nsqlite3=sqlite3\n${TEARDOWN}`);
  const panes: number[] = [];
  return {
    dir,
    home,
    calls,
    teardown: () => {
      // A group that is gone: a finished child's, whose pid is not reused within this test.
      const gone = spawnSync("sh", ["-c", "echo $$"], { encoding: "utf8" });
      const run = spawnSync("sh", [script, gone.stdout.trim()], {
        env: { ...process.env, PATH: `${join(dir, "bin")}:${process.env.PATH}` },
        encoding: "utf8",
        cwd: "/",
      });
      assert.equal(run.status, 0, run.stderr);
    },
    pane: (cpdHome) => {
      // The shell backgrounds the node, so that it is reparented and never a zombie of this process.
      const run = spawnSync("sh", ["-c", '"$0" -e "Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0)" </dev/null >/dev/null 2>&1 & echo $!', process.execPath], {
        env: { ...process.env, CPD_HOME: cpdHome },
        encoding: "utf8",
      });
      const pid = Number(run.stdout.trim());
      panes.push(pid);
      return pid;
    },
    alive: (pid) => {
      try {
        process.kill(pid, 0);
        return true;
      } catch {
        return false;
      }
    },
    stop: () => {
      server?.close();
      for (const pid of panes) {
        try {
          process.kill(pid, "SIGKILL");
        } catch {
          // already stopped
        }
      }
      rmSync(dir, { recursive: true, force: true });
      rmSync(calls, { force: true });
    },
  };
}

test("the teardown stops the processes of the env's tmux panes when list-panes fails, and no other", async () => {
  const e = await env(true);
  try {
    const pane = e.pane(e.home);
    const other = e.pane(join(e.dir, "another-env", "cpd-home"));
    assert.ok(e.alive(pane) && e.alive(other));
    e.teardown();
    assert.ok(readFileSync(e.calls, "utf8").includes("list-panes"), "the fake tmux was asked for the panes and failed");
    assert.equal(e.alive(pane), false, "a pane of the env's tmux outlived the teardown");
    assert.equal(e.alive(other), true, "the teardown stopped a process of another env");
  } finally {
    e.stop();
  }
});

test("the teardown does not stop a process whose CPD_HOME only starts with the env's (a prefix of another env's path)", async () => {
  const e = await env(true);
  try {
    const longer = e.pane(`${e.home}-2`);
    const below = e.pane(join(e.home, "below"));
    const own = e.pane(e.home);
    assert.ok(e.alive(longer) && e.alive(below) && e.alive(own));
    e.teardown();
    assert.equal(e.alive(own), false, "a pane of the env's tmux outlived the teardown");
    assert.equal(e.alive(longer), true, "the teardown stopped a process of an env whose home continues the env's name");
    assert.equal(e.alive(below), true, "the teardown stopped a process whose CPD_HOME is below the env's");
  } finally {
    e.stop();
  }
});

test("the teardown leaves the processes of an env that never had a tmux server to the group and the recorded pids", async () => {
  const e = await env(false);
  try {
    const escaped = e.pane(e.home);
    e.teardown();
    assert.equal(existsSync(e.calls), false, "tmux was not called");
    assert.equal(e.alive(escaped), true, "an unrecorded process of an env without tmux was stopped");
  } finally {
    e.stop();
  }
});
