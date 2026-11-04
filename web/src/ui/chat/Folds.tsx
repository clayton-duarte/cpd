import type { JSX } from "react";
import { memo } from "react";
import type { CompactionItem, FoldItem } from "../../protocol/domain.ts";
import { Markdown } from "./Markdown.tsx";

/** "HH:MM" (24 h, local zone) of an ISO timestamp; null when absent or invalid. */
export function formatClock(at: string | undefined): string | null {
  if (at === undefined) return null;
  const d = new Date(at);
  if (Number.isNaN(d.getTime())) return null;
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function formatTokens(n: number): string {
  return n < 1000 ? String(n) : `${Math.round(n / 100) / 10}k`;
}

export const FoldRow = memo(function FoldRow(props: { item: FoldItem }): JSX.Element {
  const { item } = props;
  const time = formatClock(item.at);
  return (
    <details data-kind="fold" data-fold={item.foldKind} open={item.defaultOpen} className="group">
      <summary
        data-part="marker"
        className="flex h-6 cursor-pointer list-none items-center gap-2 rounded-[5px] px-2 text-[12px] hover:bg-row-hover [&::-webkit-details-marker]:hidden"
      >
        <span data-part="phi" aria-hidden="true" className="w-3 font-display text-[15px] font-bold text-status-decision">
          φ
        </span>
        <span data-part="source" className="font-mono text-muted-foreground">{item.foldKind}</span>
        <span data-part="marker-text" className="min-w-0 flex-1 truncate text-foreground">{item.title}</span>
        {time === null ? null : (
          <span data-part="time" className="font-mono text-meta text-fg-dim">{time}</span>
        )}
      </summary>
      <div data-part="fold-body" className="mt-1 ml-5 rounded-[10px] border border-border bg-card px-3.5 py-2.5">
        <Markdown text={item.body} />
      </div>
    </details>
  );
});

export const CompactionRow = memo(function CompactionRow(props: { item: CompactionItem }): JSX.Element {
  const { item } = props;
  const label = item.tokensBefore === undefined ? "Context compacted" : `Context compacted · from ${formatTokens(item.tokensBefore)} tokens`;
  return (
    <details data-kind="compaction" className="group">
      <summary className="flex h-6 cursor-pointer list-none items-center gap-3 text-meta text-muted-foreground [&::-webkit-details-marker]:hidden">
        <span data-part="rule" aria-hidden="true" className="h-px flex-1 bg-border" />
        <span data-part="label" className="font-mono">{label}</span>
        <span data-part="rule" aria-hidden="true" className="h-px flex-1 bg-border" />
      </summary>
      <div data-part="compaction-body" className="mt-1 rounded-[10px] border border-border bg-card px-3.5 py-2.5">
        <Markdown text={item.summary} />
      </div>
    </details>
  );
});
