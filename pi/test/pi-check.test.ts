import assert from "node:assert/strict";
import { chmodSync, cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

// hs-d355 (docs/implementing.md "Checking a pi version"): pi-check's fake `cpd` answers from `pi/test/answers.ts`, never starts a model turn,
// fails a machine without its tools, and a fake that falls behind the adapter fails it. Each run is on a scrubbed caller environment with no provider key.

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
  mkdirSync(join(dir, "scripts"));
  cpSync(join(ROOT, "scripts", "pi-check.sh"), join(dir, "scripts", "pi-check.sh"));
  cpSync(join(ROOT, "pi"), join(dir, "pi"), { recursive: true, filter: (src) => !src.includes("node_modules") });
  symlinkSync(join(ROOT, "node_modules"), join(dir, "node_modules"));
};

const run = (script: string, args: string[], cwd: string, env: Record<string, string>) =>
  spawnSync(script, args, { cwd, env, encoding: "utf8", timeout: 80_000 });

const baseEnv = (): Record<string, string> => ({ PATH: process.env.PATH ?? "", LANG: "C.UTF-8", TERM: "xterm" });

test("the fake cpd of pi-check answers every call from pi/test/answers.ts and holds no answer of its own", () => {
  const dir = temp();
  try {
    mkdirSync(join(dir, "scripts"));
    mkdirSync(join(dir, "pi", "test"), { recursive: true });
    cpSync(join(ROOT, "scripts", "pi-check.sh"), join(dir, "scripts", "pi-check.sh"));
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

test("pi-check passes against the current adapter and the pinned pi, run as the Gate runs it, and fails when the adapter requires a field its source lacks", () => {
  const dir = temp();
  try {
    copyCheckout(dir);
    const gate = () => run("scripts/pi-check.sh", ["node_modules/.bin/pi"], dir, baseEnv());

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

/** Names pi-check allows pi to see: what the caller may pass, and what pi-check sets itself. */
const ALLOWED = new Set(["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "HOME", "TMPDIR", "PI_CODING_AGENT_DIR", "PI_OFFLINE", "PI_SKIP_VERSION_CHECK", "CPD_SESSION", "CPD_HOME"]);
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

    const r = run(join(ROOT, "scripts", "pi-check.sh"), [pi], dir, { ...baseEnv(), HOME: home, ...leaks });
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
  for (const tool of ["sh", "env", "mktemp", "mkdir", "rm", "cat", "tr", "dirname", "basename", "grep", "tail", "head", "sed", "awk", "sleep", "uname", "uuidgen", "chmod", "ls", "kill", "cp", "mv", "ln", "date", "id", "wc", "sort", "tmux", "node"]) {
    if (without.includes(tool)) continue;
    try {
      symlinkSync(lookup(tool), join(tools, tool));
    } catch {
      // a tool the machine lacks is no tool of the test
    }
  }
  return tools;
};

for (const tool of ["tmux", "node"]) {
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
