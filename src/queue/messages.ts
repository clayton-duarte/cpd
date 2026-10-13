import { z } from "zod";

/** Contract §10: message kinds. */
export const MESSAGE_KINDS = [
  "job-merged",
  "attempt-retry",
  "job-failed",
  "escalation",
  "escalation-answered",
  "review-needed",
  "all-merged",
  "pr-feedback",
  "workflow-done",
  "error",
] as const;

export const MessageKind = z.enum(MESSAGE_KINDS);
export type MessageKind = z.infer<typeof MessageKind>;

/** Contract §10: one file in `leads/<session-id>/queue/`. */
export const LeadMessageSchema = z.strictObject({
  v: z.literal(1),
  id: z.string().min(1),
  kind: MessageKind,
  wf: z.string().min(1),
  job: z.string().min(1).optional(),
  attempt: z.string().min(1).optional(),
  text: z.string().min(1),
  data: z.record(z.string(), z.unknown()),
  createdAt: z.string().min(1),
});
export type LeadMessage = z.infer<typeof LeadMessageSchema>;
