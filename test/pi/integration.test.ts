import { test } from "node:test";
import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { LeadMessageSchema, type LeadMessage } from "../../src/queue/messages.ts";
import { fakeBin } from "../helpers/index.ts";
import { agentStatus, EXTENSION, herdr, readJsonl, screen, scratch, startLead, waitFor, type Lead } from "./it-helpers.ts";

/** Needs a running `herdr --session cpd-test server` and a real pi model: only with CPD_IT=1. */
const IT = { skip: process.env.CPD_IT !== "1", timeout: 360_000 };

const userMessages = (lead: Lead) => readJsonl(lead.sessionPath).filter((e) => e.type === "message" && e.message.role === "user");
const customMessages = (lead: Lead, customType: string) => readJsonl(lead.sessionPath).filter((e) => e.type === "custom_message" && e.customType === customType);

const customEntries = (lead: Lead, customType: string) => readJsonl(lead.sessionPath).filter((e) => e.type === "custom" && e.customType === customType);

/** Write one queue message the way `notifyLead` does: `.tmp`, then rename. */
function enqueue(queueDir: string, over: Partial<LeadMessage> & Pick<LeadMessage, "kind" | "text">): LeadMessage {
  const message = LeadMessageSchema.parse({ v: 1, id: `it-${Math.random().toString(36).slice(2)}`, wf: "pay-4521", data: {}, createdAt: new Date().toISOString(), ...over });
  const path = join(queueDir, `${Date.now()}-${message.id}.json`);
  mkdirSync(queueDir, { recursive: true });
  writeFileSync(`${path}.tmp`, JSON.stringify(message));
  renameSync(`${path}.tmp`, path);
  return message;
}

test("CPD_IT: queue messages reach the Lead draft-safely; escalations set herdr blocked; invalid files are rejected", IT, async () => {
  const root = scratch("lead-");
  const cpdHome = join(root, "cpd");
  const cwd = join(root, "cwd");
  mkdirSync(cwd);
  const lead = await startLead({ name: "cpditpi1", cwd, sessionDir: join(root, "sessions"), env: { CPD_HOME: cpdHome }, piArgs: ["-e", EXTENSION] });
  try {
    assert.ok(lead.argv.includes(EXTENSION));
    // The queue dir is named by the session id pi exposes to extensions; it must equal the session file's header id.
    const queueDir = join(cpdHome, "leads", lead.sessionId, "queue");
    await waitFor("the queue dir", () => existsSync(queueDir), 20_000);

    // 1. draft safety (t1 F6)
    herdr("pane", "send-text", lead.pane, "DRAFT-PI-A1");
    await waitFor("the draft on screen", () => screen(lead.pane).includes("DRAFT-PI-A1"), 10_000);
    const merged = enqueue(queueDir, { kind: "job-merged", job: "j", text: "Job j was merged into cpd/pay-4521. Reply with just ACK-J." });
    const delivered = await waitFor("the cpd custom_message", () => customMessages(lead, "cpd").find((e) => e.details?.id === merged.id), 90_000);
    assert.equal(delivered.content, merged.text);
    assert.equal(delivered.display, true);
    assert.deepEqual(readdirSync(queueDir), [], "the file was claimed and deleted");
    assert.equal(readJsonl(lead.sessionPath)[0].id, lead.sessionId, "session header id equals the id in the queue path");

    await waitFor("the Lead to go idle", () => agentStatus(lead.name) === "idle", 90_000);
    assert.ok(screen(lead.pane).includes("DRAFT-PI-A1"), "the draft is still in the editor");
    for (const m of userMessages(lead)) assert.doesNotMatch(JSON.stringify(m), /DRAFT-PI-A1|was merged into/, "no user message carries the draft or the notification");

    // 2. an invalid file is rejected and reported
    writeFileSync(join(queueDir, "1-bad.json"), "{not json");
    const err = await waitFor("the error message", () => customMessages(lead, "cpd").find((e) => /invalid queue message 1-bad\.json/.test(e.content)), 90_000);
    assert.equal(err.details.kind, "error");
    assert.deepEqual(readdirSync(join(queueDir, "rejected")), ["1-bad.json"]);
    await waitFor("the Lead to go idle", () => agentStatus(lead.name) === "idle", 90_000);

    // 3. blocked state, counted per escalation id
    const q1 = enqueue(queueDir, { kind: "escalation", attempt: "j-a1", text: "Worker j-a1 asks X. Reply with just ACK-Q.", data: { id: "j-a1-q1" } });
    const q2 = enqueue(queueDir, { kind: "escalation", attempt: "k-a1", text: "Worker k-a1 asks Y. Reply with just ACK-Q.", data: { id: "k-a1-q1" } });
    await waitFor("herdr blocked", () => agentStatus(lead.name) === "blocked", 60_000);
    assert.ok(q1 && q2);
    enqueue(queueDir, { kind: "escalation-answered", attempt: "j-a1", text: "Escalation j-a1-q1 answered.", data: { id: "j-a1-q1" } });
    await waitFor("the answer to be consumed", () => readdirSync(queueDir).every((n) => n === "rejected"), 20_000);
    await new Promise((r) => setTimeout(r, 3_000));
    assert.equal(agentStatus(lead.name), "blocked", "one question is still open");
    enqueue(queueDir, { kind: "escalation-answered", attempt: "k-a1", text: "Escalation k-a1-q1 answered.", data: { id: "k-a1-q1" } });
    await waitFor("blocked to clear", () => agentStatus(lead.name) !== "blocked", 60_000);
    assert.equal(customMessages(lead, "cpd").filter((e) => e.details?.kind === "escalation-answered").length, 0, "escalation-answered produces no message");
  } finally {
    herdr("workspace", "close", lead.workspace);
  }
});

const PLANNOTATOR = "npm:@plannotator/pi-extension@0.28.17";

test("CPD_IT: cpd_plan_review drives plannotator; decisions reach cpd and the Lead", IT, async () => {
  const root = scratch("plan-");
  const cpdHome = join(root, "cpd");
  const cwd = join(root, "cwd");
  mkdirSync(cwd);
  const statusFile = join(homedir(), ".pi", "plannotator-review-status.json");
  const statusExisted = existsSync(statusFile);

  mkdirSync(join(root, "bin"));
  const cpd = fakeBin(
    "cpd",
    `if [ "$1" = plan ] && [ "$2" = render ]; then
  out=""; while [ $# -gt 0 ]; do [ "$1" = --out ] && out="$2"; shift; done
  printf '# Plan\\n\\n| key | title |\\n|---|---|\\n| a | Add a |\\n' > "$out"
  printf '{"ok":true,"path":"%s","fingerprint":"f"}\\n' "$out"
else
  printf '{"ok":true}\\n'
fi`,
    join(root, "bin"),
  );
  const browserLog = join(root, "browser.log");
  const browser = join(root, "browser.sh");
  writeFileSync(browser, `#!/bin/sh\necho "$1" >> '${browserLog}'\n`);
  chmodSync(browser, 0o755);

  const lead = await startLead({
    name: "cpditpi2",
    cwd,
    sessionDir: join(root, "sessions"),
    env: {
      CPD_HOME: cpdHome,
      PLANNOTATOR_BROWSER: browser,
      PLANNOTATOR_DATA_DIR: join(root, "plannotator"),
      PLANNOTATOR_REMOTE: "0",
      PLANNOTATOR_UPDATE_CHECK: "0",
    },
    piArgs: ["-e", PLANNOTATOR, "-e", EXTENSION],
    pathPrefix: cpd.dir,
  });
  try {
    const leadDir = join(cpdHome, "leads", lead.sessionId);
    await waitFor("the queue dir", () => existsSync(join(leadDir, "queue")), 20_000);
    writeFileSync(join(leadDir, "workflow"), "pay-4521"); // what `cpd lead attach` writes

    const review = async (n: number, decide: (url: string, serverSession: string) => Promise<void>) => {
      herdr("agent", "prompt", lead.name, "Call the cpd_plan_review tool exactly once, with no arguments. Then reply with just WAITING and stop.", "--wait", "--timeout", "120000");
      const url = await waitFor("the plannotator URL", () => {
        const urls = existsSync(browserLog) ? readFileSync(browserLog, "utf8").split("\n").filter(Boolean) : [];
        return urls.length >= n ? urls[n - 1] : undefined;
      }, 60_000);
      const plan = (await (await fetch(`${url}/api/plan`)).json()) as { plan: string; serverSession: string };
      assert.match(plan.plan, /Add a/, "plannotator shows the rendered plan");
      await decide(url, plan.serverSession);
    };
    const post = async (url: string, path: string, body: unknown) => {
      const r = await fetch(`${url}${path}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
      assert.equal((await r.json() as { ok: boolean }).ok, true);
    };

    // approve
    await review(1, (url, serverSession) => post(url, "/api/approve", { serverSession, planSave: { enabled: false } }));
    const approved = await waitFor("the approved decision message", () => customMessages(lead, "cpd-plan-decision").find((e) => /APPROVED/.test(e.content)), 60_000);
    assert.match(approved.content, /cpd plan lock --wf pay-4521/);
    const calls = cpd.calls();
    const render = calls.find((c) => c[0] === "plan" && c[1] === "render");
    assert.deepEqual(render?.slice(0, 2), ["plan", "render"]);
    const reviewId = approved.details.reviewId as string;
    const decisions = calls.filter((c) => c[0] === "plan" && c[1] === "decision");
    assert.deepEqual(decisions, [
      ["plan", "decision", "--review-id", reviewId, "--pending", "--wf", "pay-4521"],
      ["plan", "decision", "--review-id", reviewId, "--approved", "--wf", "pay-4521"],
    ]);

    // reject with feedback
    await waitFor("the Lead to go idle", () => agentStatus(lead.name) === "idle", 90_000);
    await review(2, (url, serverSession) => post(url, "/api/deny", { serverSession, feedback: "please add a rollback step", planSave: { enabled: false } }));
    const rejected = await waitFor("the rejected decision message", () => customMessages(lead, "cpd-plan-decision").find((e) => /REJECTED/.test(e.content)), 60_000);
    assert.match(rejected.content, /please add a rollback step/);
    const last = cpd.calls().filter((c) => c[0] === "plan" && c[1] === "decision").at(-1)!;
    const fileAt = last.indexOf("--feedback-file");
    assert.deepEqual(last.slice(0, 5), ["plan", "decision", "--review-id", rejected.details.reviewId, "--rejected"]);
    assert.ok(fileAt > 0);
    assert.match(readFileSync(last[fileAt + 1]!, "utf8"), /please add a rollback step/);

    // recovery: /reload replaces the adapter and plannotator emits no result event to it (t4 F5), so only the review-status poll can find the decision
    await waitFor("the Lead to go idle", () => agentStatus(lead.name) === "idle", 90_000);
    await review(3, async (url, serverSession) => {
      // plannotator appends a state entry at every session_start, so a new one proves the reload happened
      const starts = () => customEntries(lead, "plannotator").length;
      const before = starts();
      herdr("pane", "send-text", lead.pane, "/reload");
      herdr("pane", "send-keys", lead.pane, "enter");
      await waitFor("the reload", () => starts() > before, 30_000);
      await new Promise((r) => setTimeout(r, 5_000));
      await post(url, "/api/approve", { serverSession, feedback: "approved after reload", planSave: { enabled: false } });
    });
    const recovered = await waitFor(
      "the decision recovered after reload",
      () => customMessages(lead, "cpd-plan-decision").find((e) => /APPROVED/.test(e.content) && e.details?.reviewId !== reviewId),
      60_000,
    );
    assert.match(recovered.content, /approved|APPROVED/);
    assert.ok(cpd.calls().some((c) => c[1] === "decision" && c[3] === recovered.details.reviewId && c.includes("--approved")));

    for (const m of userMessages(lead)) assert.doesNotMatch(JSON.stringify(m), /APPROVED|REJECTED/, "the decision is delivered as a custom message, not typed");
  } finally {
    herdr("workspace", "close", lead.workspace);
    if (!statusExisted) rmSync(statusFile, { force: true });
  }
});
