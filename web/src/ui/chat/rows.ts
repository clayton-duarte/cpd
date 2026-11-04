import type { ToolItem, TranscriptItem } from "../../protocol/domain.ts";

export type Row = { kind: "item"; key: string; item: TranscriptItem } | { kind: "tools"; key: string; tools: readonly ToolItem[] };

export function groupRows(_items: readonly TranscriptItem[]): Row[] {
  throw new Error("not implemented");
}
