import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

// pi-ci-G4: the adapter is split by concern and its types carry what a cast used to assert.
const lib = join(dirname(fileURLToPath(import.meta.url)), "..", "lib");
const files = readdirSync(lib).filter((f) => f.endsWith(".ts"));
const lines = (f: string) => readFileSync(join(lib, f), "utf8").split("\n").length;

test("no module of pi/lib casts to string", () => {
  const casts = files.filter((f) => / as string\b/.test(readFileSync(join(lib, f), "utf8")));
  assert.deepEqual(casts, []);
});

test("a Run is built with its follower, not filled in after a cast", () => {
  const casts = files.filter((f) => /as unknown as Follower/.test(readFileSync(join(lib, f), "utf8")));
  assert.deepEqual(casts, []);
});

test("adapter.ts only wires the concerns together", () => {
  assert.ok(lines("adapter.ts") <= 120, `adapter.ts has ${lines("adapter.ts")} lines`);
  for (const f of ["startup.ts", "handle.ts", "review.ts", "agent-state.ts", "reload-command.ts"]) assert.ok(files.includes(f), `${f} is missing`);
});
