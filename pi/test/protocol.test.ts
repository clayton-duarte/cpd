import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { installedIdentity } from "../lib/cpd.ts";
import { LEAD_MESSAGES } from "../lib/delivery.ts";
import { KINDS, PROTOCOL } from "../lib/event.ts";
import { ev, Fixture, planShow, waitFor } from "./fake.ts";

const KIND_RS = new URL("../../core/src/events/kind.rs", import.meta.url);
const source = () => readFileSync(KIND_RS, "utf8");

test("the adapter's PROTOCOL equals core's", () => {
  const m = /pub const PROTOCOL: u32 = (\d+);/.exec(source());
  assert.ok(m, "core/src/events/kind.rs has no `pub const PROTOCOL: u32 = N;`");
  assert.equal(PROTOCOL, Number(m[1]), "pi/lib/event.ts PROTOCOL differs from core's: raise both together");
});

test("the adapter's KINDS are exactly core's Event kinds", () => {
  const block = /kinds! \{([^}]*)\}/.exec(source());
  assert.ok(block, "core/src/events/kind.rs has no `kinds! { … }` block");
  const core = [...block[1]!.matchAll(/=\s*"([a-z-]+)"\s*,/g)].map((m) => m[1]!);
  assert.ok(core.length > 0, "no kinds read from core");
  assert.deepEqual([...KINDS].sort(), [...core].sort(), "pi/lib/event.ts KINDS differs from core's kinds: update it, and raise PROTOCOL");
});

test("hs-d496: the protocol is 11, and review-opened and review-closed are known kinds that are delivered to nobody", () => {
  assert.equal(PROTOCOL, 11);
  for (const kind of ["review-opened", "review-closed"]) {
    assert.ok((KINDS as readonly string[]).includes(kind), `${kind} is not in KINDS`);
    assert.equal((LEAD_MESSAGES as Record<string, unknown>)[kind], undefined, `${kind}: delivery none`);
  }
});

// ---- the adapter's check (commands.md §15.2, step 0) ----

const mismatches = (f: Fixture) => f.pi.errors().filter((e) => (e.message.details as any).code === "protocol_mismatch");

async function started(f: Fixture) {
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

async function restart(f: Fixture) {
  const n = f.follows().length;
  f.file("exit-next", "0");
  await waitFor("a restart", () => f.follows().length === n + 1);
}

const version = (protocol?: unknown) => ({ ok: true, version: "0.0.0", schema: 6, store_schema: 6, ...(protocol === undefined ? {} : { protocol }) });

test("equal protocol: no message, and the check runs at start and at every restart", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await restart(f);
  await f.quiet(); // `restart` returns once the next follower started, after its `--version` check
  assert.equal(f.versions(), 2);
  assert.deepEqual(f.pi.sent, []);
});

test("a different protocol: one message that names both numbers and /reload; following goes on", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("--version", version(PROTOCOL + 1));
  await started(f);
  const [m] = await waitFor("the message", () => (mismatches(f).length ? mismatches(f) : undefined));
  assert.equal(
    m!.message.content,
    `cpd adapter: protocol mismatch: protocol_mismatch: cpd speaks protocol ${PROTOCOL + 1}, this adapter ${PROTOCOL}; reload pi (/reload)`,
  );
  assert.deepEqual(m!.message.details, { kind: "error", code: "protocol_mismatch", cpd: PROTOCOL + 1, adapter: PROTOCOL });
  f.emit(ev(101, "job-merged", { data: { turn: true } }));
  await waitFor("an Event delivered", () => f.pi.delivered().length === 1);
});

test("a different protocol twice for the same binary: still one message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("--version", version(PROTOCOL + 1));
  await started(f);
  await restart(f);
  await restart(f);
  await f.quiet(); // `restart` returns once the next follower started, after its `--version` check
  assert.equal(f.versions(), 3);
  assert.equal(mismatches(f).length, 1);
});

test("a new binary that is still different: the same message is not shown again (§15.1)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("--version", version(PROTOCOL + 1));
  await started(f);
  await waitFor("the first message", () => mismatches(f).length === 1);
  f.reinstall();
  await restart(f);
  await restart(f);
  await f.quiet(); // `restart` returns once the next follower started, after its `--version` check
  assert.equal(mismatches(f).length, 1);
});

test("a `--version` without protocol is a message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("--version", version());
  await started(f);
  const [m] = await waitFor("the message", () => (mismatches(f).length ? mismatches(f) : undefined));
  assert.match(String(m!.message.content), new RegExp(String.raw`^cpd adapter: protocol mismatch: protocol_mismatch: cpd speaks no protocol .*this adapter ${PROTOCOL}; reload pi \(\/reload\)$`));
  assert.equal((m!.message.details as any).cpd, null);
  await restart(f);
  await f.quiet(); // `restart` returns once the next follower started, after its `--version` check
  assert.equal(mismatches(f).length, 1);
});

test("a failing `--version` is an ordinary error once per code; following goes on", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("--version", { ok: false, error: { code: "boom", message: "it broke.", details: {} } }, { exit: 1 });
  await started(f);
  await restart(f);
  await f.quiet(); // `restart` returns once the next follower started, after its `--version` check
  assert.equal(f.pi.errors().filter((e) => (e.message.details as any).code === "boom").length, 1);
  assert.equal(mismatches(f).length, 0);
});

test("installedIdentity changes with the file and is undefined without a cpd", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const before = installedIdentity();
  assert.match(before ?? "", /\/bin\/cpd:/);
  f.reinstall();
  assert.notEqual(installedIdentity(), before);
  assert.equal(installedIdentity("/nonexistent"), undefined);
});
