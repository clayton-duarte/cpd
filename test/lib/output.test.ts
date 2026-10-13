import { test } from "node:test";
import assert from "node:assert/strict";
import { failure, notFound, refused, timeout, usage, CpdError } from "../../src/lib/errors.ts";
import { emit, errorResult } from "../../src/lib/output.ts";

function capture(fn: () => void): { lines: string[]; exit: number | string | null | undefined } {
  const orig = process.stdout.write;
  const prevExit = process.exitCode;
  let out = "";
  process.stdout.write = ((s: string) => ((out += s), true)) as typeof process.stdout.write;
  try {
    fn();
  } finally {
    process.stdout.write = orig;
  }
  const exit = process.exitCode;
  process.exitCode = prevExit;
  return { lines: out.split("\n").slice(0, -1), exit };
}

test("emit ok: one JSON line, exit 0", () => {
  const r = capture(() => emit({ ok: true, x: 1 }));
  assert.deepEqual(r.lines.map((l) => JSON.parse(l)), [{ ok: true, x: 1 }]);
  assert.equal(r.exit, 0);
});

const cases: [string, (c: string, m: string, d?: Record<string, unknown>) => CpdError, number][] = [
  ["failure", failure, 1],
  ["usage", usage, 2],
  ["refused", refused, 3],
  ["notFound", notFound, 4],
  ["timeout", timeout, 5],
];
for (const [name, make, exitCode] of cases) {
  test(`emit ${name}: one JSON line, exit ${exitCode}`, () => {
    const r = capture(() => emit(errorResult(make("some_code", "msg.", { k: 1 }))));
    assert.equal(r.lines.length, 1);
    assert.deepEqual(JSON.parse(r.lines[0]!), { ok: false, error: { code: "some_code", message: "msg.", details: { k: 1 } } });
    assert.equal(r.exit, exitCode);
  });
}
