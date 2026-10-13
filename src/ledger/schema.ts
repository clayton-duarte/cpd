import { z } from "zod";

// Shapes of `bd` 1.3.1 `--json` output that cpd reads (trial t2 F9). Loose objects: bd may add fields.

/** Dependency row as printed by `bd list` / `bd ready` / `bd children`. `blocks` is a Job Dependency. */
export const BdDependency = z.looseObject({
  issue_id: z.string(),
  depends_on_id: z.string(),
  type: z.string(),
});
export type BdDependency = z.infer<typeof BdDependency>;

export const BdIssue = z.looseObject({
  id: z.string(),
  title: z.string(),
  description: z.string().optional(),
  status: z.string(),
  issue_type: z.string(),
  created_at: z.string(),
  closed_at: z.string().optional(),
  close_reason: z.string().optional(),
  parent: z.string().optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
  dependencies: z.array(BdDependency).optional(),
});
export type BdIssue = z.infer<typeof BdIssue>;
export const BdIssues = z.array(BdIssue);

/** `bd history <id> --events --json` row. `old_value` / `new_value` are JSON text. */
export const BdEvent = z.looseObject({
  id: z.string(),
  issue_id: z.string(),
  event_type: z.string(),
  actor: z.string(),
  old_value: z.string().optional(),
  new_value: z.string().optional(),
  comment: z.string().optional(),
  created_at: z.string(),
});
export type BdEvent = z.infer<typeof BdEvent>;
export const BdEvents = z.array(BdEvent);

export const BdComment = z.looseObject({
  id: z.string(),
  issue_id: z.string(),
  author: z.string(),
  text: z.string(),
  created_at: z.string(),
});
export type BdComment = z.infer<typeof BdComment>;
export const BdComments = z.array(BdComment);

/** `bd create --json` prints the created issue; only the id is needed. */
export const BdCreated = z.looseObject({ id: z.string() });
