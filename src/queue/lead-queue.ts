import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { failure, notFound, refused } from "../lib/errors.ts";
import { workflowJsonPath } from "../lib/paths.ts";
import { LeadMessageSchema, type LeadMessage, type MessageKind } from "./messages.ts";

/** Contract §6. Only the parts the queue needs are pinned; other keys are tolerated. */
const WorkflowFile = z.object({
  v: z.literal(1),
  id: z.string().min(1),
  lead: z
    .object({
      paneId: z.string(),
      sessionPath: z.string(),
      sessionId: z.string().min(1),
      queueDir: z.string().min(1),
      attachedAt: z.string(),
    })
    .nullable(),
});
export type WorkflowFile = z.infer<typeof WorkflowFile>;

export function readWorkflowFile(wf: string): WorkflowFile {
  const path = workflowJsonPath(wf);
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") throw notFound("workflow_not_found", `workflow not found: ${wf}`, { wf, path });
    throw failure("workflow_unreadable", `cannot read ${path}: ${(e as Error).message}`, { path });
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    throw failure("workflow_invalid", `workflow.json is not valid JSON: ${path}`, { path, parseError: (e as Error).message });
  }
  const parsed = WorkflowFile.safeParse(raw);
  if (!parsed.success) throw failure("workflow_invalid", `workflow.json does not match the schema: ${path}`, { path, issues: parsed.error.issues });
  return parsed.data;
}

/** Queue dir of the attached Lead, else `lead_not_attached`. */
export function leadQueueDirOf(wf: string): string {
  const w = readWorkflowFile(wf);
  if (!w.lead) throw refused("lead_not_attached", `workflow ${wf} has no attached Lead`, { wf });
  return w.lead.queueDir;
}

export type NotifyInput = {
  kind: MessageKind;
  text: string;
  data?: Record<string, unknown>;
  job?: string;
  attempt?: string;
};

/** Write one message atomically (`.tmp` then rename). Returns the message and its path. */
export function notifyLead(wf: string, input: NotifyInput): { message: LeadMessage; path: string } {
  const queueDir = leadQueueDirOf(wf);
  const message = LeadMessageSchema.parse({
    v: 1,
    id: randomUUID(),
    kind: input.kind,
    wf,
    ...(input.job !== undefined ? { job: input.job } : {}),
    ...(input.attempt !== undefined ? { attempt: input.attempt } : {}),
    text: input.text,
    data: input.data ?? {},
    createdAt: new Date().toISOString(),
  });
  mkdirSync(queueDir, { recursive: true });
  const path = join(queueDir, `${Date.now()}-${message.id}.json`);
  const tmp = `${path}.tmp`;
  writeFileSync(tmp, JSON.stringify(message, null, 2) + "\n", { flag: "wx" });
  renameSync(tmp, path);
  return { message, path };
}
