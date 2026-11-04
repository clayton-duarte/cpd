import type { ToolItem, TranscriptItem } from "../../protocol/domain.ts";

export type Row = { kind: "item"; key: string; item: TranscriptItem } | { kind: "tools"; key: string; tools: readonly ToolItem[] };

export function groupRows(items: readonly TranscriptItem[]): Row[] {
  const rows: Row[] = [];
  let i = 0;
  while (i < items.length) {
    const item = items[i]!;
    if (item.kind !== "tool") {
      rows.push({ kind: "item", key: item.id, item });
      i++;
      continue;
    }
    const tools: ToolItem[] = [item];
    while (i + tools.length < items.length && items[i + tools.length]!.kind === "tool") tools.push(items[i + tools.length] as ToolItem);
    rows.push(tools.length > 1 ? { kind: "tools", key: `group:${item.id}`, tools } : { kind: "item", key: item.id, item });
    i += tools.length;
  }
  return rows;
}
