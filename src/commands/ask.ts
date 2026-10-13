import { parseArgs } from "node:util";
import { setTimeout as sleep } from "node:timers/promises";
import { loadConfig } from "../lib/config.ts";
import { timeout, usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { assertAttemptId, assertWorkflowId } from "../lib/paths.ts";
import { leadQueueDirOf, notifyLead } from "../queue/lead-queue.ts";
import { nextEscalationId, readAnswer, expireQuestion, writeQuestion } from "../queue/escalation-files.ts";

const POLL_MS = 500;

export async function run(argv: string[]): Promise<Result> {
  let parsed;
  try {
    parsed = parseArgs({
      args: argv,
      allowPositionals: true,
      options: { wf: { type: "string" }, "timeout-minutes": { type: "string" } },
    });
  } catch (e) {
    throw usage("usage", (e as Error).message);
  }
  const [attempt, question, ...extra] = parsed.positionals;
  if (attempt === undefined || question === undefined || extra.length > 0) {
    throw usage("usage", "usage: cpd ask <attempt-id> <question> [--timeout-minutes N] --wf <wf>");
  }
  assertAttemptId(attempt);
  if (question.trim() === "") throw usage("usage", "question must not be empty");
  const wf = parsed.values.wf ?? process.env.CPD_WF;
  if (!wf) throw usage("workflow_required", "--wf <id> (or CPD_WF) is required");
  assertWorkflowId(wf);

  let minutes: number;
  const flag = parsed.values["timeout-minutes"];
  if (flag !== undefined) {
    minutes = Number(flag);
    if (!Number.isFinite(minutes) || minutes <= 0) throw usage("usage", `invalid --timeout-minutes: ${flag}`);
  } else {
    minutes = loadConfig().askTimeoutMinutes;
  }

  leadQueueDirOf(wf); // fail lead_not_attached before leaving a question behind

  let id = "";
  for (;;) {
    id = nextEscalationId(wf, attempt);
    const q = { v: 1 as const, id, wf, attempt, question, askedAt: new Date().toISOString(), askerPid: process.pid };
    if (writeQuestion(wf, q)) break;
  }

  try {
    notifyLead(wf, {
      kind: "escalation",
      attempt,
      data: { id, question },
      text: `Worker ${attempt} is blocked and asks: ${question} Answer with: cpd answer ${id} "<answer>" --wf ${wf}`,
    });
  } catch (e) {
    expireQuestion(wf, id);
    throw e;
  }

  const deadline = Date.now() + minutes * 60_000;
  for (;;) {
    const a = readAnswer(wf, id);
    if (a) return { ok: true, id, answer: a.answer };
    if (Date.now() >= deadline) {
      expireQuestion(wf, id);
      throw timeout("ask_timeout", `no answer to ${id} within ${minutes} minutes`, { id, wf, minutes });
    }
    await sleep(POLL_MS);
  }
}
