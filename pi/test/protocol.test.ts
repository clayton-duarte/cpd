import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { installedIdentity } from "../lib/cpd.ts";
import { LEAD_MESSAGES } from "../lib/delivery.ts";
import { KINDS, PROTOCOL } from "../lib/event.ts";
import { ev, Fixture, ID, planShow, waitFor } from "./fake.ts";

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
  for (const kind of ["incident-started", "incident-ended"]) {
    assert.ok(core.includes(kind), `core's kinds are missing ${kind}`);
    assert.ok((KINDS as readonly string[]).includes(kind), `KINDS is missing ${kind}`);
  }
  assert.deepEqual([...KINDS].sort(), [...core].sort(), "pi/lib/event.ts KINDS differs from core's kinds: update it, and raise PROTOCOL");
});

test("hs-d634, pro-d8: the protocol is 15, the incident kinds and mcp-attached are known", () => {
  assert.equal(PROTOCOL, 26); // protocol-breach (D14, air-d119; it replaces bug-breach, the guards' Ledger leg runs session report with new flags) raised it to 26; the guard Event kind raised it to 25 (Plan agent-refusals); ui-origin raised it to 24 for session-message's optional data.origin; the Lead-less shaping phase raised it to 23; workflow-reopened raised it to 22; agent keys raised it to 21; the Workflow rename raised it to 20; context pending's plan-level jobs raised it to 19; session-model-changed raised it to 18; workflow-described raised it to 17; hs-d634 raised it from 13 to 14 for incident-started and incident-ended; pro-d8 raised it to 15 with mcp-attached
  for (const kind of ["incident-started", "incident-ended", "mcp-attached"]) {
    assert.ok((KINDS as readonly string[]).includes(kind), `${kind} is not in KINDS`);
  }
  assert.equal((LEAD_MESSAGES as Record<string, unknown>)["mcp-attached"], undefined, "mcp-attached is not a LEAD_MESSAGES entry: delivery none");
  assert.ok((KINDS as readonly string[]).includes("job-addendum"), "job-addendum is not in KINDS");
  assert.equal((LEAD_MESSAGES as Record<string, unknown>)["job-addendum"], undefined, "job-addendum is not a LEAD_MESSAGES entry: a Plan Lead gets none");
  assert.ok((KINDS as readonly string[]).includes("session-resumed"), "session-resumed is not in KINDS");
  // the message exists only while the breaker is tripped (commands.md §15.15 L6): the adapter's own rule, not a table entry
  assert.equal((LEAD_MESSAGES as Record<string, unknown>)["session-resumed"], undefined, "session-resumed is not a LEAD_MESSAGES entry");
  for (const kind of ["review-opened", "review-closed"]) {
    assert.ok((KINDS as readonly string[]).includes(kind), `${kind} is not in KINDS`);
    assert.equal((LEAD_MESSAGES as Record<string, unknown>)[kind], undefined, `${kind}: delivery none`);
  }
});

// ---- the adapter's check (commands.md §15.2, step 0) ----

/** A Lead's adapter errors are reported to the Manager, not delivered (commands.md §15.1, hs-d573). */
const mismatches = (f: Fixture) => f.reports().filter((r) => r.code === "protocol_mismatch");

async function started(f: Fixture) {
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

async function restart(f: Fixture) {
  const n = f.follows().length;
  f.exitNext(0);
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
  const [m] = await waitFor("the report", () => (mismatches(f).length ? mismatches(f) : undefined));
  assert.equal(m!.what, "protocol mismatch");
  assert.equal(m!.message, `cpd speaks protocol ${PROTOCOL + 1}, this adapter ${PROTOCOL}; reload pi (/reload)`);
  assert.equal(m!.session, ID);
  assert.deepEqual(m!.details, { cpd: PROTOCOL + 1, adapter: PROTOCOL });
  assert.deepEqual(f.pi.errors(), [], "not delivered to the Lead's chat");
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
  const [m] = await waitFor("the report", () => (mismatches(f).length ? mismatches(f) : undefined));
  assert.match(String(m!.message), new RegExp(String.raw`^cpd speaks no protocol .*this adapter ${PROTOCOL}; reload pi \(\/reload\)$`));
  assert.equal(m!.details?.cpd, null);
  assert.deepEqual(f.pi.errors(), [], "not delivered to the Lead's chat");
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
  assert.deepEqual(f.reports().filter((r) => r.code === "boom").map((r) => [r.what, r.message]), [["protocol check", "it broke."]]);
  assert.deepEqual(f.pi.errors(), [], "reported to the Manager, not delivered to the Lead's chat");
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

// ---- the guards' Ledger leg: every refusal reports one `protocol-breach` Event (D14, air-d119; commands.md §10.18) ----

/** The `cpd session report --kind protocol-breach` calls a refusal makes, flags read by name. */
const breaches = (f: Fixture) =>
  f
    .argvs()
    .filter((a) => a[0] === "session" && a[1] === "report" && a.includes("--kind"))
    .map((argv) => {
      const flag = (n: string) => (argv.includes(n) ? argv[argv.indexOf(n) + 1] : undefined);
      return { kind: flag("--kind"), protocol: flag("--protocol"), source: flag("--source"), tool: flag("--tool"), command: flag("--command"), session: flag("--session") };
    });

/** The Session's directory, as the adapter reads it at a tool call (the guard needs a real primary checkout). */
const inCwd = (f: Fixture, dir: string) => {
  const ctx = f.pi.ctx.bind(f.pi);
  f.pi.ctx = () => ({ ...ctx(), cwd: dir });
};

const call = async (f: Fixture, toolName: string, input: unknown) =>
  (await f.pi.fireWith("tool_call", { toolName, toolCallId: "t", input }))[0] as { block?: boolean; reason?: string } | undefined;

test("a host repair guard emits a ProtocolBreach", async (t) => {
  const tmp = mkdtempSync(join(tmpdir(), "protocol-breach-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "repo");
  mkdirSync(join(primary, ".git"), { recursive: true }); // a primary checkout: `.git` is a directory
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  inCwd(f, tmp);
  const command = `git -C ${primary} commit -m x`;
  const r = await call(f, "bash", { command });
  assert.equal(r?.block, true, "the mainguard refuses the primary-checkout write");
  await f.quiet();
  const [b] = breaches(f);
  assert.ok(b, `no \`protocol-breach\` report; the session reports are ${JSON.stringify(f.reports().map((x) => x.argv))}`);
  assert.equal(b!.kind, "protocol-breach");
  assert.equal(b!.protocol, "primary checkout");
  assert.equal(b!.source, "mainguard");
  assert.equal(b!.tool, "bash");
  assert.equal(b!.command, command);
  assert.equal(b!.session, ID);
});

test("a secret guard emits a ProtocolBreach", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  const command = "op item get x";
  const r = await call(f, "bash", { command });
  assert.equal(r?.block, true, "the secretguard refuses the raw `op` call");
  await f.quiet();
  const [b] = breaches(f);
  assert.ok(b, `no \`protocol-breach\` report; the session reports are ${JSON.stringify(f.reports().map((x) => x.argv))}`);
  assert.equal(b!.kind, "protocol-breach");
  assert.equal(b!.protocol, "secrets");
  assert.equal(b!.source, "secretguard");
  assert.equal(b!.tool, "bash");
  assert.equal(b!.command, command);
  assert.equal(b!.session, ID);
});
