import { appendFileSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { parseArgs } from "node:util";
import { z } from "zod";
import { paneGet } from "../herdr/panes.ts";
import { readSessionHeader } from "../herdr/session.ts";
import { failure, notFound, refused, usage } from "../lib/errors.ts";
import { withLock } from "../lib/lock.ts";
import type { Result } from "../lib/output.ts";
import {
  assertWorkflowId,
  cpdLogPath,
  leadQueueDir,
  leadWorkflowPath,
  ledgerLockPath,
  workflowJsonPath,
} from "../lib/paths.ts";

/** Contract §6: `lead` = `{paneId, sessionPath, sessionId, queueDir, attachedAt}`. */
const Lead = z.strictObject({
  paneId: z.string(),
  sessionPath: z.string(),
  sessionId: z.string(),
  queueDir: z.string(),
  attachedAt: z.string(),
});
type Lead = z.infer<typeof Lead>;
/** Only what this command reads; every other workflow.json key is preserved as is. */
const WorkflowFile = z.looseObject({ v: z.literal(1), id: z.string(), lead: Lead.nullable() });

function readWorkflow(wf: string): z.infer<typeof WorkflowFile> {
  const path = workflowJsonPath(wf);
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") throw notFound("workflow_not_found", `no such workflow: ${wf}`, { wf, path });
    throw e;
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw failure("workflow_corrupt", `workflow.json is not valid JSON: ${path}`, { path });
  }
  const parsed = WorkflowFile.safeParse(raw);
  if (!parsed.success) throw failure("workflow_corrupt", `workflow.json does not match the schema: ${path}`, { path, issues: parsed.error.issues });
  return parsed.data;
}

/** Write `content` atomically: temp file in the same directory, then rename. */
function writeAtomic(path: string, content: string): void {
  mkdirSync(dirname(path), { recursive: true });
  const tmp = `${path}.${process.pid}.tmp`;
  writeFileSync(tmp, content);
  renameSync(tmp, path);
}

function logEvent(wf: string, event: string, fields: Record<string, unknown>): void {
  const path = cpdLogPath(wf);
  mkdirSync(dirname(path), { recursive: true });
  appendFileSync(path, JSON.stringify({ at: new Date().toISOString(), level: "info", event, ...fields }) + "\n");
}

/** Contract §11: bind the calling Lead session (pane, session file, queue) to a Workflow. */
export async function run(argv: string[]): Promise<Result> {
  let values: { wf?: string };
  try {
    ({ values } = parseArgs({ args: argv, options: { wf: { type: "string" } }, allowPositionals: false, strict: true }));
  } catch (e) {
    throw usage("usage", `cpd lead attach: ${(e as Error).message}`);
  }
  if (process.env.HERDR_ENV !== "1") {
    throw refused("not_in_herdr", "cpd lead attach must run inside a herdr pane (HERDR_ENV=1)");
  }
  const wf = values.wf ?? process.env.CPD_WF;
  if (!wf) throw usage("workflow_required", "pass --wf <id> (or set CPD_WF)");
  assertWorkflowId(wf);
  const paneId = process.env.HERDR_PANE_ID;
  if (!paneId) throw refused("not_in_herdr", "HERDR_PANE_ID is not set; cpd lead attach must run inside a herdr pane");

  const pane = await paneGet(paneId);
  const sessionPath = pane.agent_session?.value;
  if (!sessionPath) {
    throw failure("lead_session_unknown", `herdr reports no agent session for pane ${paneId}; run this from the Lead's pi session`, { paneId });
  }
  const sessionId = readSessionHeader(sessionPath).id;
  const lead: Lead = { paneId, sessionPath, sessionId, queueDir: leadQueueDir(sessionId), attachedAt: new Date().toISOString() };

  await withLock(ledgerLockPath(wf), "lead attach", () => {
    const workflow = readWorkflow(wf);
    mkdirSync(lead.queueDir, { recursive: true });
    writeAtomic(leadWorkflowPath(sessionId), wf);
    writeAtomic(workflowJsonPath(wf), JSON.stringify({ ...workflow, lead }, null, 2) + "\n");
    logEvent(wf, "lead_attached", {
      sessionId,
      paneId,
      replaced: workflow.lead && workflow.lead.sessionId !== sessionId ? workflow.lead.sessionId : null,
    });
  });
  return { ok: true, wf, lead };
}
