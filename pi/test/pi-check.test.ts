import assert from "node:assert/strict";
import { chmodSync, cpSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

// hs-d355 (docs/implementing.md "Checking a pi version"): pi-check's fake `cpd` answers from `pi/test/answers.ts`, never starts a model turn,
// fails a machine without its tools, and a fake that falls behind the adapter fails it. Each run is on a scrubbed caller environment with no provider key.
// The script runs pi on a pty (scripts/pi-check-pty.py), never in a multiplexer, and these tests use no real pi, no real wait and no real multiplexer:
// the pi is a node script of the test that writes what it saw to files (its pid, its tty size), and the test reads the script's exit status.

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

const temp = (): string => mkdtempSync(join(tmpdir(), "pi-check-test-"));
const write = (file: string, text: string): string => {
  writeFileSync(file, text);
  chmodSync(file, 0o755);
  return file;
};
const lookup = (tool: string): string => {
  const found = (process.env.PATH ?? "").split(delimiter).map((d) => join(d, tool)).find((p) => spawnSync("test", ["-x", p]).status === 0);
  assert.ok(found, `${tool} on PATH`);
  return found;
};

/** A copy of the checkout's pi-check and adapter in `dir`, with the real node_modules: the Gate's layout. */
const copyCheckout = (dir: string): void => {
  copyScripts(dir);
  cpSync(join(ROOT, "pi"), join(dir, "pi"), { recursive: true, filter: (src) => !src.includes("node_modules") });
};

/** The script and its pty runner. */
const copyScripts = (dir: string): void => {
  mkdirSync(join(dir, "scripts"), { recursive: true });
  for (const name of ["pi-check.sh", "pi-check-pty.py"]) cpSync(join(ROOT, "scripts", name), join(dir, "scripts", name));
};

const run = (script: string, args: string[], cwd: string, env: Record<string, string>) =>
  spawnSync(script, args, { cwd, env, encoding: "utf8", timeout: 80_000 });

const baseEnv = (): Record<string, string> => ({ PATH: process.env.PATH ?? "", LANG: "C.UTF-8", TERM: "xterm" });

test("the fake cpd of pi-check answers every call from pi/test/answers.ts and holds no answer of its own", () => {
  const dir = temp();
  try {
    mkdirSync(join(dir, "pi", "test"), { recursive: true });
    copyScripts(dir);
    // A source that answers each call with its own argv: whatever pi-check's fake prints came through it.
    writeFileSync(join(dir, "pi", "test", "answers.ts"), `console.log(JSON.stringify({ ok: true, from: "answers.ts", argv: process.argv.slice(2) }));\n`);
    const out = join(dir, "out");
    mkdirSync(out);
    // Stands for pi: asks the fake cpd what the adapter asks at activation, then follows, which is what pi-check waits for.
    const pi = write(
      join(dir, "fake-pi"),
      `#!/bin/sh
case " $* " in
  *" --version "*) echo "fake-pi 1.0"; exit 0 ;;
  *" --mode rpc "*) exit 0 ;;
esac
cpd session show "$CPD_SESSION" >'${out}/session'
cpd policy show >'${out}/policy'
cpd --version >'${out}/version'
cpd plan show p1 >'${out}/plan'
exec cpd events follow --session "$CPD_SESSION" --since 0
`,
    );

    const r = run(join(dir, "scripts", "pi-check.sh"), [pi], dir, baseEnv());

    assert.equal(r.status, 0, `${r.stdout}${r.stderr}`);
    assert.match(r.stdout, /pi-check: ok \(/);
    const seen = (name: string) => JSON.parse(readFileSync(join(out, name), "utf8")) as { from?: string; argv?: string[] };
    for (const [name, argv] of [["policy", ["policy", "show"]], ["version", ["--version"]], ["plan", ["plan", "show", "p1"]]] as const) {
      assert.deepEqual(seen(name), { ok: true, from: "answers.ts", argv }, `\`cpd ${argv.join(" ")}\` was not answered by pi/test/answers.ts`);
    }
    const session = seen("session");
    assert.equal(session.from, "answers.ts", "`cpd session show` was not answered by pi/test/answers.ts");
    assert.equal(session.argv?.slice(0, 2).join(" "), "session show");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** A fake pi, a node script: answers `--version` and the rpc load, runs `activation` (JS in an async function, with `args`, `fs` and `cp` in scope) at the activation, then follows as the adapter does. */
const nodePi = (dir: string, activation = ""): string =>
  write(
    join(dir, "fake-pi"),
    `#!/usr/bin/env node
const fs = require("node:fs");
const cp = require("node:child_process");
const args = process.argv.slice(2);
if (args.includes("--version")) { console.log("fake-pi 1.0"); process.exit(0); }
if (args.includes("--mode")) process.exit(0);
(async () => {
${activation}
cp.spawnSync("cpd", ["events", "follow", "--session", process.env.CPD_SESSION, "--since", "0"], { stdio: "ignore" });
})();
`,
  );

/** The adapter's own `parseSession` (the copy in `dir`) over what the fake `cpd` answers to `session show`: a pi that follows only when the adapter accepts the answer. */
const adapterPi = (dir: string): string =>
  nodePi(
    dir,
    `const { parseSession } = await import(${JSON.stringify(join(dir, "pi", "lib", "run.ts"))});
  const shown = JSON.parse(cp.execFileSync("cpd", ["session", "show", process.env.CPD_SESSION], { encoding: "utf8" }));
  try { parseSession(shown); } catch (e) { console.error(String(e)); process.exit(1); }`,
  );

test("pi-check passes while the adapter accepts what its fake cpd answers, and fails when the adapter requires a field its source lacks", () => {
  const dir = temp();
  try {
    copyCheckout(dir);
    const gate = () => run("scripts/pi-check.sh", [adapterPi(dir)], dir, baseEnv());

    const ok = gate();
    assert.equal(ok.status, 0, `pi-check on the current adapter: ${ok.stdout}${ok.stderr}`);
    assert.match(ok.stdout, /pi-check: ok \(/);

    // The adapter gets ahead of the fake: it starts requiring a field that answers.ts does not have.
    const file = join(dir, "pi", "lib", "run.ts");
    const source = readFileSync(file, "utf8");
    const ahead = source.replace('typeof s.role !== "string" ||', 'typeof s.role !== "string" || typeof s.a_field_the_fake_lacks !== "string" ||');
    assert.notEqual(ahead, source, "the adapter's parseSession changed; update this test");
    writeFileSync(file, ahead);

    const behind = gate();
    assert.equal(behind.status, 1, `${behind.stdout}${behind.stderr}`);
    assert.match(behind.stderr, /pi-check: FAIL: step 3 \(activation\)/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** What the fake pi of the pty tests wrote in `out`. */
type Seen = { pid: number; child: number; columns: number; rows: number; stdinTTY: boolean; stdoutTTY: boolean };
const seen = (out: string): Seen => JSON.parse(readFileSync(join(out, "seen"), "utf8")) as Seen;
/** The state of `pid` as `ps -o stat=` prints it, or undefined when there is no such process. */
const psState = (pid: number): string | undefined => {
  const state = spawnSync("ps", ["-o", "stat=", "-p", String(pid)], { encoding: "utf8" }).stdout.trim();
  return state === "" ? undefined : state;
};
/**
 * Whether `pid` is still running. A killed process whose parent is gone stays a zombie (state Z) until init reaps it, and `kill(pid, 0)` succeeds on a
 * zombie (macOS, under load: the incident of the Gate of PR clk), so the state decides: no process, or Z, is gone. Every assertion below goes through here.
 */
const alive = (pid: number, stateOf: (pid: number) => string | undefined = psState): boolean => {
  const state = stateOf(pid);
  return state !== undefined && !state.startsWith("Z");
};

test("alive: a zombie and a process that does not exist are gone, a live process (whatever its state) is not", () => {
  const reaped = spawnSync(process.execPath, ["-e", ""]).pid;
  assert.equal(alive(reaped), false, "a reaped process");
  assert.equal(alive(process.pid), true, "this process, really running");
  assert.equal(alive(process.pid, () => "Z"), false, "a zombie: kill(pid, 0) succeeds on it, yet it is gone");
  assert.equal(alive(process.pid, () => "Z+"), false, "a zombie that holds a tty");
  for (const state of ["R", "S", "S+", "Ss", "U", "T", "I"]) assert.equal(alive(process.pid, () => state), true, `state ${state} is a process that outlived the check`);
});

/** A fake pi that records its pid, its tty and a child of its own that never ends (stands for what pi starts), into `out/seen`, then runs `after` (JS). */
const recordingPi = (dir: string, out: string, after = ""): string =>
  nodePi(
    dir,
    `const child = cp.spawn(process.execPath, ["-e", "setInterval(() => {}, 2 ** 30)"], { stdio: "ignore" });
  fs.writeFileSync(${JSON.stringify(join(out, "seen"))}, JSON.stringify({ pid: process.pid, child: child.pid, columns: process.stdout.columns, rows: process.stdout.rows, stdinTTY: process.stdin.isTTY === true, stdoutTTY: process.stdout.isTTY === true }));
  ${after}`,
  );

test("pi runs on a 120x30 tty, and when the check passes pi and what it started are gone", () => {
  const dir = temp();
  try {
    const out = join(dir, "out");
    mkdirSync(out);
    const r = run(join(ROOT, "scripts", "pi-check.sh"), [recordingPi(dir, out)], dir, baseEnv());

    assert.equal(r.status, 0, `${r.stdout}${r.stderr}`);
    const s = seen(out);
    assert.deepEqual([s.stdinTTY, s.stdoutTTY, s.columns, s.rows], [true, true, 120, 30]);
    assert.equal(alive(s.pid), false, "pi outlived the check");
    assert.equal(alive(s.child), false, "a child of pi outlived the check");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a pi that exits before it follows fails step 3 at once, says how it exited, and leaves nothing running", () => {
  const dir = temp();
  try {
    const out = join(dir, "out");
    mkdirSync(out);
    const r = run(join(ROOT, "scripts", "pi-check.sh"), [recordingPi(dir, out, "process.exit(3);")], dir, baseEnv());

    assert.equal(r.status, 1, `${r.stdout}${r.stderr}`);
    assert.match(r.stderr, /pi-check: FAIL: step 3 \(activation\).*pi exited with status 3/);
    assert.doesNotMatch(r.stdout, /pi-check: ok/);
    assert.equal(alive(seen(out).child), false, "a child of pi outlived the check");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a pi that never follows fails step 3 at the check's own limit (PI_CHECK_ACTIVATION_SECS=0: no wait), and leaves nothing running", () => {
  const dir = temp();
  try {
    const out = join(dir, "out");
    mkdirSync(out);
    const r = run(join(ROOT, "scripts", "pi-check.sh"), [recordingPi(dir, out, "setInterval(() => {}, 2 ** 30);\n  await new Promise(() => {});")], dir, { ...baseEnv(), PI_CHECK_ACTIVATION_SECS: "0" });

    assert.equal(r.status, 1, `${r.stdout}${r.stderr}`);
    assert.match(r.stderr, /pi-check: FAIL: step 3 \(activation\)/);
    const s = (() => {
      try {
        return seen(out);
      } catch {
        return undefined; // pi was stopped before it wrote
      }
    })();
    if (s) assert.equal(alive(s.pid), false, "pi outlived the check");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("SIGTERM during the activation stops pi and what it started, removes the temporary directory and exits 130", () => {
  const dir = temp();
  try {
    const out = join(dir, "out");
    mkdirSync(out);
    const base = scratchTmp(dir);
    // The pi signals its parent, the pty runner, as a terminating signal would, then hangs: only the runner's handler ends it.
    const pi = recordingPi(dir, out, 'process.kill(process.ppid, "SIGTERM");\n  setInterval(() => {}, 2 ** 30);\n  await new Promise(() => {});');
    const r = run(join(ROOT, "scripts", "pi-check.sh"), [pi], dir, { ...baseEnv(), TMPDIR: base });

    assert.equal(r.status, 130, `${r.stdout}${r.stderr}`);
    assert.doesNotMatch(`${r.stdout}${r.stderr}`, /pi-check: (ok|FAIL)/);
    const s = seen(out);
    assert.equal(alive(s.pid), false, "pi outlived the check");
    assert.equal(alive(s.child), false, "a child of pi outlived the check");
    assert.deepEqual(leftovers(base), [], "the temporary directory is removed");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** Names pi-check allows pi to see: what the caller may pass, and what pi-check sets itself. */
const ALLOWED = new Set(["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "HOME", "TMPDIR", "PI_CODING_AGENT_DIR", "GATE_RUN", "PI_OFFLINE", "PI_SKIP_VERSION_CHECK", "CPD_SESSION", "CPD_HOME"]);
const SHELL_ADDS = new Set(["PWD", "SHLVL", "_"]);

test("no provider credential, cloud credential, proxy or base URL reaches any pi command of pi-check, whatever the caller's environment holds", () => {
  const dir = temp();
  try {
    const log = join(dir, "env.log");
    const home = join(dir, "caller-home");
    mkdirSync(home);
    // Stands for pi: records its environment and what the agent directory and HOME hold; then follows in interactive mode so pi-check can end.
    const pi = write(
      join(dir, "fake-pi"),
      `#!/bin/sh
{ echo "--- $*"; env; echo "--- files"; ls -A "$PI_CODING_AGENT_DIR" "$HOME" 2>&1; } >>'${log}'
case " $* " in
  *" --version "*) echo "fake-pi 1.0"; exit 0 ;;
  *" --mode rpc "*) exit 0 ;;
esac
exec cpd events follow --session "$CPD_SESSION" --since 0
`,
    );
    const leaks: Record<string, string> = {
      ANTHROPIC_API_KEY: "sk-leak", OPENAI_API_KEY: "sk-leak", GEMINI_API_KEY: "leak", GOOGLE_API_KEY: "leak", FOO_API_KEY: "leak", SOME_BASE_URL: "http://leak",
      HTTP_PROXY: "http://leak", HTTPS_PROXY: "http://leak", ALL_PROXY: "http://leak", NO_PROXY: "leak", http_proxy: "http://leak", https_proxy: "http://leak",
      AWS_ACCESS_KEY_ID: "leak", GOOGLE_APPLICATION_CREDENTIALS: "/leak.json", PI_SOMETHING_ELSE: "leak", CPD_PLAN: "leak", CPD_ATTEMPT: "leak",
    };

    const r = run(join(ROOT, "scripts", "pi-check.sh"), [pi], dir, { ...baseEnv(), HOME: home, GATE_RUN: "abc123", ...leaks });
    assert.equal(r.status, 0, `${r.stdout}${r.stderr}`);

    const blocks = readFileSync(log, "utf8").split(/^--- /m).filter(Boolean).map((b) => b.split("\n"));
    const calls = blocks.filter((b) => !b[0]!.startsWith("files"));
    assert.ok(calls.length >= 3, `pi ran ${calls.length} times; pi-check runs it for the version, the load and the activation`);
    for (const call of calls) {
      const names = new Map<string, string>();
      for (const line of call.slice(1)) {
        const eq = line.indexOf("=");
        if (eq > 0 && /^[A-Za-z_][A-Za-z0-9_]*$/.test(line.slice(0, eq))) names.set(line.slice(0, eq), line.slice(eq + 1));
      }
      for (const name of names.keys()) assert.ok(ALLOWED.has(name) || SHELL_ADDS.has(name), `\`${call[0]}\`: ${name} crossed into pi's environment`);
      assert.equal(names.get("PI_OFFLINE"), "1", call[0]);
      assert.equal(names.get("GATE_RUN"), "abc123", `${call[0]}: the Gate's run id does not reach pi, so its leak check cannot see the process`);
      assert.notEqual(names.get("HOME"), home, `${call[0]}: HOME is the caller's`);
      if (!call[0]!.includes("--session-id")) for (const name of names.keys()) assert.ok(!name.startsWith("CPD_"), `\`${call[0]}\`: ${name} is set outside the activation`);
    }
    const activation = calls.find((c) => c[0]!.includes("--session-id"));
    assert.ok(activation, "the activation step ran pi");
    assert.ok(activation.some((l) => l.startsWith("CPD_SESSION=")) && activation.some((l) => l.startsWith("CPD_HOME=")), "the activation has CPD_SESSION and CPD_HOME");

    const files = blocks.filter((b) => b[0]!.startsWith("files")).flatMap((b) => b.slice(1));
    for (const forbidden of ["auth.json", "settings.json", "models.json"]) assert.ok(!files.includes(forbidden), `the temporary HOME or agent directory holds ${forbidden}`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** A directory of symlinks to the tools pi-check needs, except those in `without`: a `PATH` on which they are missing. */
const toolsWithout = (dir: string, without: string[]): string => {
  const tools = join(dir, "tools");
  mkdirSync(tools);
  for (const tool of ["sh", "env", "mktemp", "mkdir", "rm", "cat", "tr", "dirname", "basename", "grep", "tail", "head", "sed", "awk", "mkfifo", "uname", "uuidgen", "chmod", "ls", "kill", "cp", "mv", "ln", "date", "id", "wc", "sort", "python3", "node"]) {
    if (without.includes(tool)) continue;
    try {
      symlinkSync(lookup(tool), join(tools, tool));
    } catch {
      // a tool the machine lacks is no tool of the test
    }
  }
  return tools;
};

for (const tool of ["python3", "node"]) {
  test(`a machine without ${tool} fails step 0 of pi-check, never skips and never says ok`, () => {
    const dir = temp();
    try {
      const pi = write(join(dir, "fake-pi"), `#!/bin/sh\ncase " $* " in *" --version "*) echo "fake-pi 1.0" ;; esac\nexit 0\n`);
      const tools = toolsWithout(dir, [tool]);
      const r = run(join(ROOT, "scripts", "pi-check.sh"), [pi], dir, { ...baseEnv(), PATH: tools });
      assert.equal(r.status, 1, `${r.stdout}${r.stderr}`);
      assert.match(r.stderr, new RegExp(`pi-check: FAIL: step 0 \\(tools\\): ${tool} is not installed \\(step 3 needs it\\)`));
      assert.doesNotMatch(r.stdout, /pi-check: ok/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
}

// hs-d463 (docs/implementing.md "Checking a pi version", "The cleanup cannot fail a pass"): on macOS pi-check printed `pi-check: ok`,
// then its `rm -rf "$tmp"` failed `Directory not empty` (a child of pi's pane still wrote into it), and the exit
// status became the cleanup's: the check failed after a pass and the pi pin did not move. The exit status is the check's.

const realRm = (): string => lookup("rm");

/** A fake pi that passes (it follows, as pi-check waits for), after `activation` (shell code, run only at the activation step). */
const passingPi = (dir: string, activation = ""): string =>
  write(
    join(dir, "fake-pi"),
    `#!/bin/sh
case " $* " in
  *" --version "*) echo "fake-pi 1.0"; exit 0 ;;
  *" --mode rpc "*) exit 0 ;;
esac
${activation}
exec cpd events follow --session "$CPD_SESSION" --since 0
`,
  );

/** A directory first on `PATH` with a fake `rm` that fails its first `failures` calls like macOS's (`Directory not empty`), then removes. */
const flakyRm = (dir: string, failures: number): string => {
  const bin = join(dir, "fakebin");
  mkdirSync(bin);
  const count = join(dir, "rm.count");
  write(
    join(bin, "rm"),
    `#!/bin/sh
n=$(cat '${count}' 2>/dev/null || echo 0); n=$((n + 1)); echo $n >'${count}'
for last; do :; done
if [ "$n" -le ${failures} ]; then echo "rm: $last: Directory not empty" >&2; exit 1; fi
exec '${realRm()}' "$@"
`,
  );
  return bin;
};

/** What a run of pi-check leaves in `base`, its TMPDIR: the `pi-check.*` directories. */
const leftovers = (base: string): string[] => readdirSync(base).filter((n) => n.startsWith("pi-check."));

/** A scratch TMPDIR for pi-check, so the temp directory it makes is found and the test owns it. */
const scratchTmp = (dir: string): string => {
  const base = join(dir, "tmpbase");
  mkdirSync(base);
  return base;
};

test("a cleanup whose rm fails twice with `Directory not empty` is retried, and the pass stays a pass", () => {
  const dir = temp();
  try {
    const base = scratchTmp(dir);
    const bin = flakyRm(dir, 2);
    const r = run(join(ROOT, "scripts", "pi-check.sh"), [passingPi(dir)], dir, { ...baseEnv(), TMPDIR: base, PI_CHECK_CLEANUP_PAUSE: "0", PATH: `${bin}${delimiter}${process.env.PATH ?? ""}` });

    assert.match(r.stdout, /pi-check: ok \(/);
    assert.equal(r.status, 0, `the check passed and its cleanup failed it: ${r.stdout}${r.stderr}`);
    assert.deepEqual(leftovers(base), [], "the temporary directory is removed once rm stops failing");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a cleanup that always fails only warns: the pass stays a pass, with no FAIL line", () => {
  const dir = temp();
  try {
    const base = scratchTmp(dir);
    const bin = flakyRm(dir, 1_000_000);
    const r = run(join(ROOT, "scripts", "pi-check.sh"), [passingPi(dir)], dir, { ...baseEnv(), TMPDIR: base, PI_CHECK_CLEANUP_SECS: "1", PI_CHECK_CLEANUP_PAUSE: "0", PATH: `${bin}${delimiter}${process.env.PATH ?? ""}` });

    assert.match(r.stdout, /pi-check: ok \(/);
    assert.equal(r.status, 0, `the check passed and its cleanup failed it: ${r.stdout}${r.stderr}`);
    assert.match(r.stderr, /pi-check: warning: could not remove \S*pi-check\.\w+: .*Directory not empty/);
    assert.doesNotMatch(`${r.stdout}${r.stderr}`, /pi-check: FAIL/, "a cleanup never prints FAIL");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a failed check whose cleanup also fails still fails: FAIL, the warning, exit 1, never 0", () => {
  const dir = temp();
  try {
    const base = scratchTmp(dir);
    const bin = flakyRm(dir, 1_000_000);
    const pi = write(join(dir, "fake-pi"), `#!/bin/sh\necho "fake-pi: broken" >&2\nexit 3\n`);
    const r = run(join(ROOT, "scripts", "pi-check.sh"), [pi], dir, { ...baseEnv(), TMPDIR: base, PI_CHECK_CLEANUP_SECS: "1", PI_CHECK_CLEANUP_PAUSE: "0", PATH: `${bin}${delimiter}${process.env.PATH ?? ""}` });

    assert.equal(r.status, 1, `${r.stdout}${r.stderr}`);
    assert.match(r.stderr, /pi-check: FAIL: step 1 \(version\)/);
    assert.match(r.stderr, /pi-check: warning: could not remove \S*pi-check\.\w+: /);
    assert.doesNotMatch(r.stdout, /pi-check: ok/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
