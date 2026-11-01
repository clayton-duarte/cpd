import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { test } from "node:test";
import { rustWallClockPatterns, scansWhole } from "./rust-scan.ts";

// pi-ci-F3: a test that waits a fixed time for something that must not happen is slow on an idle machine and still wrong on a loaded one.
// It waits for the fact it checks (`waitFor`, `Fixture.drain`, `Fixture.quiet`), with a deadline and a message.
test("no pi test sleeps a fixed time: no `settle`, no `sleep(` in a test file", () => {
  const found: string[] = [];
  for (const name of readdirSync(import.meta.dirname).filter((n) => n.endsWith(".ts") && n !== "no-fixed-sleeps.test.ts")) {
    readFileSync(`${import.meta.dirname}/${name}`, "utf8").split("\n").forEach((line, i) => {
      const code = line.replace(/\/\/.*$/, "");
      if (/\bsettle\s*\(|\bconst settle\b/.test(code) || (name.endsWith(".test.ts") && /\bsleep\s*\(/.test(code))) found.push(`${name}:${i + 1}: ${line.trim()}`);
    });
  }
  assert.deepEqual(found, [], "fixed sleeps in pi/test");
});

// Static sleep check (docs/implementing.md, Tests, point 4): debts are listed per file in pi/test/sleep-allow.txt.
// The patterns are built from parts so that this file holds none of the scanned text.
const TAGS = ["m-units", "m-session", "m-engine", "m-tui-r", "m-tui-t", "m-flow", "m-pr-a", "m-pr-b", "m-tooling", "contract", "m-pi"];
const ROOT = join(import.meta.dirname, "..", "..");
const stripComment = (line: string) => line.replace(/\/\/.*$/, "");

const walk = (dir: string): string[] =>
  !existsSync(dir)
    ? []
    : readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
        const p = join(dir, e.name);
        return e.isDirectory() ? walk(p) : [p];
      });
const rel = (p: string) => relative(ROOT, p).split(sep).join("/");

function allowed(): Map<string, number> {
  const file = join(ROOT, "pi", "test", "sleep-allow.txt");
  const out = new Map<string, number>();
  if (!existsSync(file)) return out;
  for (const line of readFileSync(file, "utf8").split("\n")) {
    if (line.trim() === "" || line.trim().startsWith("#")) continue;
    const m = /^(\S+) (\d+)  # ([a-z0-9-]+): \S/.exec(line);
    assert.ok(m && TAGS.includes(m[3]), `malformed sleep-allow.txt line: ${line}`);
    assert.ok(!out.has(m[1]), `duplicate sleep-allow.txt path: ${m[1]}`);
    out.set(m[1], Number(m[2]));
  }
  return out;
}

function report(counts: Map<string, number>, inDomain: (path: string) => boolean): string[] {
  const allow = allowed();
  const bad: string[] = [];
  for (const path of [...new Set([...counts.keys(), ...[...allow.keys()].filter(inDomain)])].sort()) {
    const n = counts.get(path) ?? 0;
    const a = allow.get(path) ?? 0;
    if (n !== a) bad.push(`${path}: ${n} (allowed ${a})`);
  }
  return bad;
}

const countLines = (lines: string[], patterns: RegExp[]) => lines.filter((l) => patterns.some((p) => p.test(l))).length;

test("no pi test waits on a timer or the wall clock outside the allowlist", () => {
  const patterns = [
    new RegExp("\\bset" + "Timeout\\("),
    new RegExp("\\bset" + "Interval\\("),
    new RegExp("\\bDate\\." + "now\\(\\)"),
    new RegExp("spawnSync\\(['\"]" + "sleep['\"]"),
    new RegExp("\\bwait" + "For\\("),
  ];
  const files = [...walk(join(ROOT, "pi", "test")), join(ROOT, "pi", "skills", "skill.test.ts")].filter(
    (p) => p.endsWith(".ts") && rel(p) !== "pi/test/fake.ts" && existsSync(p),
  );
  const counts = new Map<string, number>();
  for (const f of files) {
    const n = countLines(readFileSync(f, "utf8").split("\n").map(stripComment), patterns);
    if (n > 0) counts.set(rel(f), n);
  }
  assert.deepEqual(report(counts, (p) => p.startsWith("pi/")), [], "timers or wall clock in pi tests (path: count (allowed n))");
});

test("no Rust test sleeps or reads the wall clock outside the allowlist", () => {
  const patterns = rustWallClockPatterns();
  const counts = new Map<string, number>();
  const scan = (path: string, lines: string[]) => {
    const n = countLines(lines.map(stripComment), patterns);
    if (n > 0) counts.set(rel(path), n);
  };
  for (const dir of ["cli/tests", "core/tests"]) {
    for (const f of walk(join(ROOT, dir)).filter((p) => p.endsWith(".rs"))) scan(f, readFileSync(f, "utf8").split("\n"));
  }
  for (const crate of ["cli", "core", "engine", "tui"]) {
    for (const f of walk(join(ROOT, crate, "src")).filter((p) => p.endsWith(".rs"))) {
      const lines = readFileSync(f, "utf8").split("\n");
      const whole = scansWhole(crate, relative(join(ROOT, crate, "src"), f).split(sep).join("/"));
      if (whole) scan(f, lines);
      else {
        const i = lines.findIndex((l) => l.trim().startsWith("#[cfg(test)]"));
        if (i >= 0) scan(f, lines.slice(i));
      }
    }
  }
  assert.deepEqual(report(counts, (p) => !p.startsWith("pi/")), [], "timers or wall clock in Rust tests (path: count (allowed n))");
});

test("the Rust wall-clock scan counts a bare sleep and a tokio sleep and spares a fake clock's method and a declaration", () => {
  const lines = [
    "    sleep(Duration::from_millis(5));",
    "    tokio::time::sleep(d).await;",
    "    clock.sleep(d);",
    "    fn sleep(&self, d: Duration) {",
    "    // sleep(d)",
  ];
  const counted = lines.filter((l) => countLines([stripComment(l)], rustWallClockPatterns()) === 1);
  assert.deepEqual(counted, [lines[0], lines[1]]);
});

test("the Rust wall-clock scan reads a test module named by its file name", () => {
  assert.equal(scansWhole("engine", "watch/wake_tests.rs"), true);
  assert.equal(scansWhole("core", "x/testkit.rs"), true);
  assert.equal(scansWhole("core", "store/mod.rs"), false);
  assert.equal(scansWhole("core", "tests.rs"), true);
  assert.equal(scansWhole("core", "a/tests/b.rs"), true);
});
