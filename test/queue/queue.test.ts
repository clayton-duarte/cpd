import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { notifyLead } from "../../src/queue/lead-queue.ts";
import { LeadMessageSchema } from "../../src/queue/messages.ts";
import {
  expireOrphans, listOpenQuestions, nextEscalationId, readAnswer, readQuestion, writeAnswer, writeQuestion,
} from "../../src/queue/escalation-files.ts";
import { setup, WF } from "./helpers.ts";

test("notifyLead without an attached Lead fails lead_not_attached", () => {
  setup(false);
  assert.throws(() => notifyLead(WF, { kind: "error", text: "x" }), (e: any) => e.code === "lead_not_attached" && e.exitCode === 3);
});

test("notifyLead writes a schema-valid <epoch-ms>-<id>.json", () => {
  const { queueDir } = setup();
  const { message } = notifyLead(WF, { kind: "job-merged", text: "hi", job: "j", attempt: "j-a1", data: { a: 1 } });
  const names = readdirSync(queueDir);
  assert.deepEqual(names, [`${names[0]!.split("-")[0]}-${message.id}.json`]);
  assert.match(names[0]!, /^\d+-.+\.json$/);
  LeadMessageSchema.parse(JSON.parse(readFileSync(join(queueDir, names[0]!), "utf8")));
});

/** Runs in its own Node process, so it really races the writer. Protocol: prints "running", then a JSON report after "stop" on stdin. */
const READER = `
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
const { LeadMessageSchema } = await import(process.env.MESSAGES_URL);
const dir = process.env.QUEUE_DIR;
const problems = [];
let reads = 0;
let stop = false;
process.stdin.on("data", () => { stop = true; });
console.log("running");
while (!stop) {
  for (const n of readdirSync(dir)) {
    if (!n.endsWith(".json")) continue;
    try {
      LeadMessageSchema.parse(JSON.parse(readFileSync(join(dir, n), "utf8")));
      reads++;
    } catch (e) {
      if (e.code !== "ENOENT") problems.push(n + ": " + e.message.slice(0, 200));
    }
  }
  await new Promise((r) => setImmediate(r));
}
console.log(JSON.stringify({ problems, reads }));
process.exit(0);
`;

test("a reader process racing 200 writes never sees a partial .json; every file validates", async () => {
  const { queueDir } = setup();
  notifyLead(WF, { kind: "error", text: "seed" });
  const reader = spawn(process.execPath, ["--input-type=module", "-e", READER], {
    env: { ...process.env, QUEUE_DIR: queueDir, MESSAGES_URL: new URL("../../src/queue/messages.ts", import.meta.url).href },
    stdio: ["pipe", "pipe", "pipe"],
  });
  let out = "";
  let err = "";
  reader.stdout.on("data", (d) => (out += d));
  reader.stderr.on("data", (d) => (err += d));
  const exited = new Promise<void>((res) => reader.on("close", () => res()));
  const t0 = Date.now();
  while (!out.includes("running")) {
    if (Date.now() - t0 > 15000) throw new Error(`reader did not start: ${err}`);
    await new Promise((r) => setTimeout(r, 10));
  }
  for (let i = 0; i < 200; i++) notifyLead(WF, { kind: "pr-feedback", text: "x".repeat(200_000), data: { i } });
  reader.stdin.end("stop\n");
  await exited;
  assert.equal(err, "");
  const report = JSON.parse(out.trim().split("\n").pop()!) as { problems: string[]; reads: number };
  assert.deepEqual(report.problems, []);
  assert.ok(report.reads > 50, `reader only completed ${report.reads} reads`);
  const names = readdirSync(queueDir);
  const all = names.filter((n) => n.endsWith(".json"));
  assert.equal(all.length, 201);
  for (const n of all) LeadMessageSchema.parse(JSON.parse(readFileSync(join(queueDir, n), "utf8")));
  assert.equal(names.filter((n) => n.endsWith(".tmp")).length, 0);
});

const q = (id: string, attempt: string, pid = process.pid) => ({
  v: 1 as const, id, wf: WF, attempt, question: "why?", askedAt: new Date().toISOString(), askerPid: pid,
});

test("question/answer files round-trip; nextEscalationId counts per attempt; exclusive create", () => {
  setup();
  assert.equal(nextEscalationId(WF, "job-a1"), "job-a1-q1");
  assert.equal(writeQuestion(WF, q("job-a1-q1", "job-a1")), true);
  assert.equal(writeQuestion(WF, q("job-a1-q1", "job-a1")), false);
  assert.equal(nextEscalationId(WF, "job-a1"), "job-a1-q2");
  assert.equal(nextEscalationId(WF, "job-a2"), "job-a2-q1");
  assert.equal(readQuestion(WF, "job-a1-q1")!.question, "why?");
  assert.equal(readAnswer(WF, "job-a1-q1"), null);
  assert.equal(writeAnswer(WF, { v: 1, id: "job-a1-q1", answer: "because", answeredAt: "t" }), true);
  assert.equal(writeAnswer(WF, { v: 1, id: "job-a1-q1", answer: "again", answeredAt: "t" }), false);
  assert.equal(readAnswer(WF, "job-a1-q1")!.answer, "because");
});

test("an invalid question file fails loud", () => {
  const { escDir } = setup();
  writeQuestion(WF, q("job-a1-q1", "job-a1"));
  writeFileSync(join(escDir, "job-a1-q1.q.json"), '{"v":1}');
  assert.throws(() => readQuestion(WF, "job-a1-q1"), (e: any) => e.code === "escalation_invalid");
});

test("restart durability: an unread question is listed until answered", () => {
  setup();
  writeQuestion(WF, q("job-a1-q1", "job-a1"));
  writeQuestion(WF, q("job-a1-q2", "job-a1"));
  assert.deepEqual(listOpenQuestions(WF).map((x) => x.id), ["job-a1-q1", "job-a1-q2"]);
  writeAnswer(WF, { v: 1, id: "job-a1-q1", answer: "a", answeredAt: "t" });
  assert.deepEqual(listOpenQuestions(WF).map((x) => x.id), ["job-a1-q2"]);
});

test("expireOrphans removes an unanswered question whose asker is dead, keeps live and answered", async () => {
  const { escDir } = setup();
  const dead = await new Promise<number>((res) => {
    const c = spawn(process.execPath, ["-e", ""], { stdio: "ignore" });
    c.on("close", () => res(c.pid!));
  });
  writeQuestion(WF, q("a-a1-q1", "a-a1", dead));
  writeQuestion(WF, q("b-a1-q1", "b-a1", process.pid));
  writeQuestion(WF, q("c-a1-q1", "c-a1", dead));
  writeAnswer(WF, { v: 1, id: "c-a1-q1", answer: "x", answeredAt: "t" });
  assert.deepEqual(expireOrphans(WF), ["a-a1-q1"]);
  assert.equal(existsSync(join(escDir, "a-a1-q1.q.json")), false);
  assert.equal(existsSync(join(escDir, "a-a1-q1.expired.json")), true);
  assert.equal(nextEscalationId(WF, "a-a1"), "a-a1-q2");
  assert.deepEqual(listOpenQuestions(WF).map((x) => x.id), ["b-a1-q1"]);
  assert.equal(existsSync(join(escDir, "b-a1-q1.q.json")), true);
  assert.equal(existsSync(join(escDir, "c-a1-q1.q.json")), true);
});
