import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { MESSAGE_KINDS, type LeadMessage } from "../../src/queue/messages.ts";
import { adapterActive } from "../../pi/lib/activation.ts";
import { createBlockedCounter } from "../../pi/lib/blocked.ts";
import { callCpd } from "../../pi/lib/cpd.ts";
import { actionOf, DELIVERY, deliveryOf } from "../../pi/lib/delivery.ts";
import { createReviews, decisionText, parseRequested, parseStatus, REVIEW_ENTRY_TYPE, reviewsFromEntries } from "../../pi/lib/plan-review.ts";
import { claim, drain, processFile, type Sink } from "../../pi/lib/queue.ts";
import { tmpDir } from "../helpers/index.ts";

const msg = (over: Partial<LeadMessage> = {}): LeadMessage => ({
  v: 1, id: "m1", kind: "job-merged", wf: "pay-4521", text: "Job j merged.", data: {}, createdAt: "2026-01-01T00:00:00Z", ...over,
});
const put = (dir: string, name: string, body: unknown) => {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, name), typeof body === "string" ? body : JSON.stringify(body));
};
function recorder() {
  const messages: { msg: LeadMessage; delivery: ReturnType<typeof deliveryOf> }[] = [];
  const rejected: ReturnType<typeof deliveryOf>[] = [];
  const sink: Sink = { message: (m, d) => messages.push({ msg: m, delivery: d }), rejected: (d) => rejected.push(d) };
  return { messages, rejected, sink };
}

// --- queue: claim and validate ---

test("claim: duplicate watch events deliver a message exactly once and leave no file behind", () => {
  const dir = tmpDir("cpd-pi-q-");
  put(dir, "1-m1.json", msg());
  const r = recorder();
  assert.equal(processFile(dir, "1-m1.json", r.sink), true);
  for (let i = 0; i < 5; i++) assert.equal(processFile(dir, "1-m1.json", r.sink), false);
  assert.equal(drain(dir, r.sink), 0);
  assert.equal(r.messages.length, 1);
  assert.deepEqual(readdirSync(dir), []);
});

test("claim: only the first of two claimants gets the file", () => {
  const dir = tmpDir("cpd-pi-q-");
  put(dir, "1-m1.json", msg());
  assert.notEqual(claim(dir, "1-m1.json"), null);
  assert.equal(claim(dir, "1-m1.json"), null);
});

test("drain: delivers oldest first and ignores .tmp, rejected/ and non-json", () => {
  const dir = tmpDir("cpd-pi-q-");
  put(dir, "2-b.json", msg({ id: "b" }));
  put(dir, "1-a.json", msg({ id: "a" }));
  put(dir, "3-c.json.tmp", msg({ id: "c" }));
  mkdirSync(join(dir, "rejected"));
  const r = recorder();
  assert.equal(drain(dir, r.sink), 2);
  assert.deepEqual(r.messages.map((m) => m.msg.id), ["a", "b"]);
  assert.equal(existsSync(join(dir, "3-c.json.tmp")), true);
});

for (const [label, body, reason] of [
  ["not JSON", "{nope", /not valid JSON/],
  ["wrong schema", { v: 1, id: "x" }, /kind/],
  ["unknown kind", msg({ kind: "nope" as never }), /kind/],
  ["extra key", { ...msg(), extra: 1 }, /extra|unrecognized/i],
  ["escalation without data.id", msg({ kind: "escalation" }), /data\.id/],
] as const) {
  test(`invalid message (${label}) goes to rejected/ plus an error delivery`, () => {
    const dir = tmpDir("cpd-pi-q-");
    put(dir, "1-bad.json", body);
    const r = recorder();
    assert.equal(processFile(dir, "1-bad.json", r.sink), true);
    assert.equal(r.messages.length, 0);
    assert.equal(r.rejected.length, 1);
    const d = r.rejected[0]!;
    assert.equal(d.message.customType, "cpd");
    assert.equal(d.options.triggerTurn, true);
    assert.equal(d.options.deliverAs, "followUp");
    assert.match(d.message.content, /1-bad\.json/);
    assert.match(d.message.content, reason);
    assert.deepEqual(readdirSync(dir), ["rejected"]);
    assert.deepEqual(readdirSync(join(dir, "rejected")), ["1-bad.json"]);
    assert.equal(readFileSync(join(dir, "rejected", "1-bad.json"), "utf8"), typeof body === "string" ? body : JSON.stringify(body), "the file is kept as written");
  });
}

// --- kind -> delivery (contract §10) ---

test("delivery mapping equals the §10 table", () => {
  const table: Record<string, [deliver: boolean, triggerTurn: boolean]> = {
    "job-merged": [true, true],
    "attempt-retry": [true, false],
    "job-failed": [true, true],
    escalation: [true, true],
    "escalation-answered": [false, false],
    "review-needed": [true, true],
    "all-merged": [true, true],
    "pr-feedback": [true, true],
    "workflow-done": [true, true],
    error: [true, true],
    rename: [false, false],
  };
  assert.deepEqual([...MESSAGE_KINDS].sort(), Object.keys(table).sort());
  for (const kind of MESSAGE_KINDS) {
    const [deliver, triggerTurn] = table[kind]!;
    assert.deepEqual(DELIVERY[kind], { deliver, triggerTurn }, kind);
    const m = msg({ kind, data: { id: "j-a1-q1" } });
    const d = deliveryOf(m);
    if (!deliver) {
      assert.equal(d, null, kind);
      continue;
    }
    assert.deepEqual(d, { message: { customType: "cpd", content: m.text, display: true, details: m }, options: { triggerTurn, deliverAs: "followUp" } }, kind);
  }
});

test("escalation-answered produces no delivery through the queue", () => {
  const dir = tmpDir("cpd-pi-q-");
  put(dir, "1-a.json", msg({ kind: "escalation-answered", data: { id: "j-a1-q1" } }));
  const r = recorder();
  drain(dir, r.sink);
  assert.equal(r.messages.length, 1);
  assert.equal(r.messages[0]!.delivery, null);
  assert.deepEqual(readdirSync(dir), []);
});

// --- blocked counter ---

const esc = (kind: "escalation" | "escalation-answered", id: string) => msg({ kind, data: { id } });

test("blocked: two questions; answering one keeps blocked; answering both clears it", () => {
  const c = createBlockedCounter();
  const events = [
    c.onMessage(esc("escalation", "j-a1-q1")),
    c.onMessage(esc("escalation", "j-a2-q1")),
    c.onMessage(esc("escalation-answered", "j-a1-q1")),
  ];
  assert.deepEqual(events, [{ active: true, label: "cpd: Worker question" }, { active: true, label: "cpd: Worker question" }, { active: false }]);
  assert.equal(c.count, 1, "still blocked after one answer");
  assert.deepEqual(c.onMessage(esc("escalation-answered", "j-a2-q1")), { active: false });
  assert.equal(c.count, 0);
});

test("blocked: a duplicate or unknown id never unbalances herdr's counter", () => {
  const c = createBlockedCounter();
  assert.notEqual(c.onMessage(esc("escalation", "j-a1-q1")), null);
  assert.equal(c.onMessage(esc("escalation", "j-a1-q1")), null);
  assert.equal(c.onMessage(esc("escalation-answered", "other-a1-q1")), null);
  assert.deepEqual(c.onMessage(esc("escalation-answered", "j-a1-q1")), { active: false });
  assert.equal(c.onMessage(esc("escalation-answered", "j-a1-q1")), null);
  assert.equal(c.count, 0);
});

test("blocked: other kinds never touch it", () => {
  const c = createBlockedCounter();
  for (const kind of MESSAGE_KINDS.filter((k) => !k.startsWith("escalation"))) assert.equal(c.onMessage(msg({ kind })), null, kind);
});

test("blocked: seeded from open questions, then the queue's escalation for the same id is not counted twice", () => {
  const c = createBlockedCounter();
  assert.deepEqual(c.seed(["j-a1-q1", "j-a1-q2"]), [{ active: true, label: "cpd: Worker question" }, { active: true, label: "cpd: Worker question" }]);
  assert.equal(c.onMessage(esc("escalation", "j-a1-q1")), null);
  assert.equal(c.count, 2);
  assert.deepEqual(c.onMessage(esc("escalation-answered", "j-a1-q1")), { active: false });
  assert.equal(c.count, 1);
});

// --- plan review bookkeeping ---

test("plan review: pending to approved and to rejected, each exactly once", () => {
  const r = createReviews();
  r.markPending("r1", "pay-4521");
  r.markPending("r2", "pay-4521");
  assert.deepEqual(r.pending().map((x) => x.reviewId), ["r1", "r2"]);
  assert.deepEqual(r.settle("r1", true), { reviewId: "r1", wf: "pay-4521", status: "approved" });
  assert.equal(r.settle("r1", true), null, "duplicate result");
  assert.equal(r.settle("r1", false), null, "no flip after a decision");
  assert.deepEqual(r.settle("r2", false), { reviewId: "r2", wf: "pay-4521", status: "rejected" });
  assert.equal(r.settle("unknown", true), null, "someone else's review is ignored");
  assert.deepEqual(r.pending(), []);
});

test("plan review: a lost review leaves pending once", () => {
  const r = createReviews();
  r.markPending("r1", "pay-4521");
  assert.equal(r.lose("r1")?.status, "lost");
  assert.equal(r.lose("r1"), null);
  assert.equal(r.settle("r1", true), null);
});

const entry = (data: unknown, customType = REVIEW_ENTRY_TYPE) => ({ type: "custom", customType, data });

test("plan review: recovery from session entries keeps only the still-pending ids", () => {
  const entries = [
    { type: "message", message: { role: "user" } },
    entry({ reviewId: "r1", wf: "w-1", status: "pending" }),
    entry({ reviewId: "r2", wf: "w-1", status: "pending" }),
    entry({ reviewId: "r1", wf: "w-1", status: "approved" }),
    entry({ x: 1 }, "plannotator"),
  ];
  const r = createReviews(reviewsFromEntries(entries));
  assert.deepEqual(r.pending(), [{ reviewId: "r2", wf: "w-1", status: "pending" }]);
  assert.equal(r.get("r1")?.status, "approved");
  assert.equal(r.settle("r1", false), null, "an already-decided review is not decided again");
  assert.equal(r.settle("r2", true)?.status, "approved");
});

test("plan review: a malformed adapter entry fails loud", () => {
  assert.throws(() => reviewsFromEntries([entry({ reviewId: "r1" })]));
});

test("plannotator responses are parsed at the boundary", () => {
  assert.equal(parseRequested({ status: "handled", result: { status: "pending", reviewId: "abc" } }), "abc");
  assert.throws(() => parseRequested({ status: "unavailable", error: "no" }), /unavailable/);
  assert.throws(() => parseRequested({ status: "handled", result: { status: "completed" } }));
  assert.deepEqual(parseStatus({ status: "handled", result: { status: "completed", approved: false, feedback: "f", savedPath: "x" } }), { status: "completed", approved: false, feedback: "f", savedPath: "x" });
  assert.equal(parseStatus({ status: "handled", result: { status: "missing" } }).status, "missing");
  assert.throws(() => parseStatus({ status: "handled", result: { status: "completed" } }));
});

test("decision text: approved points at plan lock, rejected carries the feedback, a cpd failure is stated", () => {
  const ok = { reviewId: "r1", wf: "pay-4521", status: "approved" as const };
  assert.match(decisionText(ok, undefined), /APPROVED[\s\S]*cpd plan lock --wf pay-4521/);
  const no = { ...ok, status: "rejected" as const };
  const t = decisionText(no, "add a rollback step");
  assert.match(t, /REJECTED/);
  assert.match(t, /add a rollback step/);
  assert.match(decisionText(ok, undefined, "cpd plan decision failed (illegal_phase): x"), /could not record[\s\S]*illegal_phase/);
  assert.doesNotMatch(decisionText(ok, undefined, "boom"), /cpd plan lock/);
});

// --- cpd calls ---

test("callCpd: returns the parsed {ok:true} output and throws cpd's own error otherwise", async () => {
  const calls: string[][] = [];
  const exec = (out: string, code = 0) => async (c: string, a: string[]) => {
    calls.push([c, ...a]);
    return { stdout: out, stderr: "", code };
  };
  assert.deepEqual(await callCpd(exec('{"ok":true,"path":"/p"}'), ["plan", "render"]), { ok: true, path: "/p" });
  assert.deepEqual(calls, [["cpd", "plan", "render"]]);
  await assert.rejects(callCpd(exec('{"ok":false,"error":{"code":"illegal_phase","message":"nope"}}', 3), ["plan", "decision"]), /illegal_phase.*nope/);
  await assert.rejects(callCpd(exec("garbage", 1), ["x"]), /exited 1 without JSON/);
  await assert.rejects(callCpd(exec('{"ok":true}', 2), ["x"]), /exit_2/);
});

test("adapter: with no CPD_HOME set the queue dir is under ~/.cpd", async () => {
  const { leadQueueDir } = await import("../../src/lib/paths.ts");
  const saved = process.env.CPD_HOME;
  delete process.env.CPD_HOME;
  try {
    assert.equal(leadQueueDir("s1"), join(homedir(), ".cpd", "leads", "s1", "queue"));
  } finally {
    if (saved !== undefined) process.env.CPD_HOME = saved;
  }
});

test("pi package: the manifest exposes pi/skills/cpd-lead/SKILL.md next to the extension", () => {
  const root = new URL("../../pi/", import.meta.url);
  const pkg = JSON.parse(readFileSync(new URL("package.json", root), "utf8")) as { pi: { extensions: string[]; skills: string[] } };
  assert.deepEqual(pkg.pi.extensions, ["./extension.ts"]);
  assert.deepEqual(pkg.pi.skills, ["./skills"]);
  assert.ok(existsSync(new URL("skills/cpd-lead/SKILL.md", root)));
  assert.equal(existsSync(new URL("../skills/cpd-lead/SKILL.md", root)), false, "the old location is gone");
});

// --- activation ---

test("adapterActive: HERDR_ENV=1 or TMUX_PANE turns it on, neither leaves it off", () => {
  assert.equal(adapterActive({ HERDR_ENV: "1" }), true);
  assert.equal(adapterActive({ TMUX_PANE: "%1" }), true);
  assert.equal(adapterActive({}), false);
  assert.equal(adapterActive({ HERDR_ENV: "0", TMUX_PANE: "" }), false);
});

test("rename: maps to a set-name action and no delivery, so no turn is triggered", () => {
  const m = msg({ kind: "rename", wf: undefined, data: { name: "my session" } });
  assert.deepEqual(actionOf(m), { type: "set-name", name: "my session", notice: 'cpd: session renamed to "my session"' });
  assert.equal(deliveryOf(m), null);
  assert.equal(DELIVERY.rename.triggerTurn, false);
  assert.equal(actionOf(msg()), null);
});

test("rename: a message without data.name is rejected at the queue", () => {
  const dir = tmpDir("cpd-pi-q-");
  put(dir, "1-r.json", { v: 1, id: "r", kind: "rename", text: "x", data: {}, createdAt: "2026-01-01T00:00:00Z" });
  const r = recorder();
  drain(dir, r.sink);
  assert.equal(r.messages.length, 0);
  assert.equal(r.rejected.length, 1);
});
