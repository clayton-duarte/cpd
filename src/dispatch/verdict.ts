import { z } from "zod";

// Contract §12 step 9: the Reviewer's verdict is a fenced `json` block in its last assistant message.
//
// Parsing rules (the choices the contract leaves open):
// - A block is a fenced code block whose info string is exactly `json` (``` fences, at line start).
// - The LAST such block is the verdict. A Reviewer that quotes JSON while reasoning and then ends with its
//   verdict is read correctly, and a block that is not the verdict can never win over the final one.
//   Because the last block decides, a trailing JSON block after the verdict makes the message invalid (fail loud).
// - Prose before or after the block is ignored: the verdict itself is unambiguous.
// - The block must match the schema exactly: `{"verdict": "approve" | "reject", "feedback": <non-blank string>}`, no other keys.

const Verdict = z.strictObject({
  verdict: z.enum(["approve", "reject"]),
  feedback: z.string().refine((s) => s.trim() !== "", "feedback must not be blank"),
});

export type ParsedVerdict =
  | { verdict: "approve" | "reject"; feedback: string }
  | { verdict: "invalid"; reason: string };

const FENCE = /^```json[ \t]*\r?\n([\s\S]*?)^```[ \t]*$/gm;

/** The verdict in `lastAssistantText`, or `{verdict: "invalid", reason}`. Never throws. */
export function parseVerdict(lastAssistantText: string): ParsedVerdict {
  const blocks = [...lastAssistantText.matchAll(FENCE)];
  const last = blocks.at(-1);
  if (!last) return { verdict: "invalid", reason: "no fenced json block" };
  let raw: unknown;
  try {
    raw = JSON.parse(last[1]!);
  } catch (e) {
    return { verdict: "invalid", reason: `the last json block is not valid JSON: ${(e as Error).message}` };
  }
  const parsed = Verdict.safeParse(raw);
  if (!parsed.success) return { verdict: "invalid", reason: `the last json block is not a verdict: ${z.prettifyError(parsed.error)}` };
  return parsed.data;
}
