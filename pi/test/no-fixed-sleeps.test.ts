import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

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
