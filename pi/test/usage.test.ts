import assert from "node:assert/strict";
import { test } from "node:test";
import { Fixture } from "./fake.ts";
import { usageError, type Grammar } from "./usage.ts";

const g: Grammar = {
  "": { flags: { "--help": false, "--version": false }, positionals: { min: 0, max: 0 }, required: [] },
  "plan show": { flags: { "--help": false, "--plan": true }, positionals: { min: 0, max: 0 }, required: [] },
  "plan review": { flags: { "--help": false, "--plan": true, "--title": true }, positionals: { min: 0, max: 0 }, required: ["--title"] },
  "session show": { flags: { "--help": false }, positionals: { min: 0, max: 1 }, required: [] },
  "session rename": { flags: { "--help": false, "--session": true }, positionals: { min: 1, max: 1 }, required: [] },
};

test("a positional where the command takes a flag is refused", () => {
  assert.match(usageError(["plan", "show", "p1"], g) ?? "", /'p1'/);
  assert.equal(usageError(["plan", "show", "--plan", "p1"], g), undefined);
});

test("a flag the command does not have is refused", () => {
  assert.match(usageError(["plan", "show", "--frob"], g) ?? "", /'--frob'/);
  assert.equal(usageError(["plan", "show", "--plan=p1"], g), undefined);
});

test("a flag without its value is refused", () => {
  assert.match(usageError(["plan", "show", "--plan"], g) ?? "", /'--plan'/);
  assert.equal(usageError(["plan", "show", "--plan", "-x"], g), undefined);
});

test("a missing required flag is refused", () => {
  assert.match(usageError(["plan", "review", "--plan", "p1"], g) ?? "", /--title/);
  assert.equal(usageError(["plan", "review", "--plan", "p1", "--title", "t"], g), undefined);
});

test("an unknown command is refused", () => {
  assert.match(usageError(["frob"], g) ?? "", /'frob'/);
  assert.match(usageError(["plan", "frob"], g) ?? "", /'frob'/);
  assert.match(usageError(["plan"], g) ?? "", /'plan'/);
  assert.equal(usageError(["--version"], g), undefined);
});

test("an argv the grammar accepts is accepted", () => {
  assert.equal(usageError(["session", "show"], g), undefined);
  assert.equal(usageError(["session", "show", "abc"], g), undefined);
  assert.equal(usageError(["session", "rename", "a name", "--session", "s1"], g), undefined);
  assert.match(usageError(["session", "show", "a", "b"], g) ?? "", /'b'/);
  assert.equal(typeof usageError(["session", "rename", "--session", "s1"], g), "string");
  assert.match(usageError(["plan", "show", "--", "--plan"], g) ?? "", /'--plan'/);
});

test("the fake cpd answers a usage error for an argv cpd refuses", async () => {
  const f = new Fixture();
  try {
    f.respond("plan-show", { ok: true });
    const bad = await f.pi.exec("cpd", ["plan", "show", "p1"]);
    assert.equal(bad.code, 2);
    const out = JSON.parse(bad.stdout);
    assert.equal(out.ok, false);
    assert.equal(out.error.code, "usage");
    assert.match(out.error.message, /'p1'/);
    const good = await f.pi.exec("cpd", ["plan", "show", "--plan", "p1"]);
    assert.deepEqual(JSON.parse(good.stdout), { ok: true });
    assert.deepEqual(f.argvs().filter((a) => a[0] === "plan" && a[1] === "show"), [["plan", "show", "p1"], ["plan", "show", "--plan", "p1"]]);
  } finally {
    await f.close();
  }
});
