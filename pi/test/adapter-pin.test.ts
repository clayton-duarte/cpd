import assert from "node:assert/strict";
import { cpSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readlinkSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test, type TestContext } from "node:test";
import { fileURLToPath } from "node:url";
import { pinAdapter } from "../lib/adapter-pin.ts";
import { Fixture } from "./fake.ts";

const VERSION = "pi.20260101120000-1";
const OTHER = "pi.20260101130000-2";

/** A share directory (removed at the test's end) holding the version directories `names`. */
function share(t: TestContext, ...names: string[]): string {
  const dir = mkdtempSync(join(tmpdir(), "pi-pin-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  for (const n of names) mkdirSync(join(dir, n));
  return dir;
}
const pins = (dir: string) => readdirSync(dir).filter((n) => n.startsWith("pi.pin."));

test("pinAdapter writes pi.pin.<pid> beside the version directory, pointing at it by its bare name", (t) => {
  const s = share(t, VERSION);
  const path = pinAdapter(join(s, VERSION), 4242);
  assert.equal(path, join(s, "pi.pin.4242"));
  assert.ok(lstatSync(join(s, "pi.pin.4242")).isSymbolicLink());
  assert.equal(readlinkSync(join(s, "pi.pin.4242")), VERSION);
});

test("pinAdapter resolves a symlinked adapter directory to the version directory it names", (t) => {
  const s = share(t, VERSION);
  symlinkSync(VERSION, join(s, "pi"));
  const path = pinAdapter(join(s, "pi"), 4243);
  assert.equal(path, join(s, "pi.pin.4243"));
  assert.equal(readlinkSync(join(s, "pi.pin.4243")), VERSION);
});

test("pinAdapter twice for one pid leaves one pin", (t) => {
  const s = share(t, VERSION);
  pinAdapter(join(s, VERSION), 4244);
  pinAdapter(join(s, VERSION), 4244);
  assert.deepEqual(pins(s), ["pi.pin.4244"]);
  assert.equal(readlinkSync(join(s, "pi.pin.4244")), VERSION);
});

test("pinAdapter replaces a stale pin of the same pid that named another version", (t) => {
  const s = share(t, VERSION, OTHER);
  symlinkSync(OTHER, join(s, "pi.pin.4245"));
  pinAdapter(join(s, VERSION), 4245);
  assert.deepEqual(pins(s), ["pi.pin.4245"]);
  assert.equal(readlinkSync(join(s, "pi.pin.4245")), VERSION);
});

test("pinAdapter pins nothing and returns null for a directory that is not a version directory", (t) => {
  const s = share(t, "adapter", "pi.1");
  assert.equal(pinAdapter(join(s, "adapter"), 4246), null);
  assert.equal(pinAdapter(join(s, "pi.1"), 4246), null);
  assert.deepEqual(pins(s), []);
});

test("adapter() pins the version directory it was loaded from, for every role", async (t) => {
  const root = mkdtempSync(join(tmpdir(), "pi-pin-load-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const repo = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
  symlinkSync(join(repo, "node_modules"), join(root, "node_modules"));
  const sh = join(root, "share");
  mkdirSync(join(sh, VERSION), { recursive: true });
  for (const n of ["extension.ts", "package.json", "lib", "compact-tools"]) cpSync(join(repo, "pi", n), join(sh, VERSION, n), { recursive: true });
  const { adapter } = (await import(join(sh, VERSION, "lib", "adapter.ts"))) as typeof import("../lib/adapter.ts");

  const pin = join(sh, `pi.pin.${process.pid}`);
  const roles: [string, (f: Fixture) => void][] = [
    ["manager", () => { delete process.env.CPD_PLAN; }],
    ["lead", () => { process.env.CPD_PLAN = "p1"; }],
    ["worker", () => { process.env.CPD_PLAN = "p1"; process.env.CPD_ATTEMPT = "a1"; }],
  ];
  const savedAttempt = process.env.CPD_ATTEMPT;
  t.after(() => {
    if (savedAttempt === undefined) delete process.env.CPD_ATTEMPT;
    else process.env.CPD_ATTEMPT = savedAttempt;
  });
  for (const [role, env] of roles) {
    const f = new Fixture(); // saves and restores CPD_SESSION, CPD_PLAN, CPD_HOME
    try {
      env(f);
      rmSync(pin, { force: true });
      f.load({ reload: { dir: undefined } }, f.pi, adapter);
      assert.ok(lstatSync(pin, { throwIfNoEntry: false })?.isSymbolicLink(), `${role}: ${pin} is a symlink`);
      assert.equal(readlinkSync(pin), VERSION, role);
    } finally {
      await f.close();
    }
  }
});
