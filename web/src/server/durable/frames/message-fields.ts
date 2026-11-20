import { FOLD_TITLE_MAX, type FoldKind } from "../../../protocol/domain.ts";
import { isObj, obj, protocolError, str, type Obj } from "./validation.ts";

export const isoOf = (message: Obj): { at?: string } =>
  typeof message.timestamp === "number" && Number.isFinite(message.timestamp) ? { at: new Date(message.timestamp).toISOString() } : {};

/** Text blocks of a message `content` (a string as is), joined with "\n". */
export function textOf(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .filter((b): b is Obj => isObj(b) && b.type === "text" && typeof b.text === "string")
    .map((b) => b.text as string)
    .join("\n");
}

const cut = (s: string): string => s.slice(0, FOLD_TITLE_MAX);
const firstLine = (s: string): string => cut(s.split("\n")[0] ?? "");

export const FOLD_KINDS: readonly FoldKind[] = ["cpd-context", "cpd-plan-decision", "cpd-plan-summary", "cpd-skills"];

export function foldTitle(kind: FoldKind, body: string, data: Obj): string {
  switch (kind) {
    case "cpd-context":
      return typeof data.summary === "string" && data.summary !== "" ? cut(data.summary) : firstLine(body);
    case "cpd-plan-summary":
      return `Plan review ${String(data.review)} summary`;
    case "cpd-plan-decision":
      return `Plan review ${String(data.review)} ${data.approved === true ? "approved" : "rejected"}`;
    case "cpd-skills":
      return firstLine(body);
    case "cpd":
      return firstLine(body);
  }
}

// Only the producer's digest baseline may omit a model message.
export function skillsBaseline(entry: Obj): boolean {
  if (entry.kind !== "cpd-skills" || !Array.isArray(entry.model) || entry.model.length !== 0) return false;
  const data = obj(entry.data, "skills data");
  str(data.skill, "skills skill");
  const files = obj(data.files, "skills files");
  for (const digest of Object.values(files)) {
    if (typeof digest !== "string" || !/^[0-9a-f]{64}$/.test(digest)) protocolError("invalid skills digest");
  }
  return true;
}

export function skillsMessage(message: Obj): void {
  if (message.role !== "user") protocolError(`skills message: expected role user, got ${String(message.role)}`);
  if (typeof message.content === "string") return;
  if (!Array.isArray(message.content) || !message.content.every((block: unknown) => isObj(block) && block.type === "text" && typeof block.text === "string")) {
    protocolError("invalid skills message content");
  }
}
