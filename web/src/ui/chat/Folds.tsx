import { BookmarkIcon } from "lucide-react";
import type { JSX } from "react";
import { memo, useState } from "react";
import type { CompactionItem, FoldItem } from "../../protocol/domain.ts";
import { Marker, MarkerContent } from "../components/marker.tsx";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "../components/trace.tsx";
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

export const FoldRow = memo(function FoldRow(props: {
  item: FoldItem;
}): JSX.Element {
  const { item } = props;
  const [open, setOpen] = useState(Boolean(item.defaultOpen));
  const time = formatClock(item.at);
  return (
    <Trace data-kind="fold" data-fold={item.foldKind}>
      <TraceCollapsible open={open} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger data-part="marker">
          <TraceLine className="text-xs">
            <TraceIcon
              data-part="phi"
              className="font-display font-bold text-status-decision"
            >
              φ
            </TraceIcon>
            <span
              data-part="source"
              className="shrink-0 font-mono text-muted-foreground"
            >
              {item.foldKind}
            </span>
            <TraceTitle data-part="marker-text" className="text-foreground">
              {item.title}
            </TraceTitle>
            {time === null ? null : (
              <TraceDescription data-part="time" className="font-mono">
                {time}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent data-part="fold-body">
          <Markdown text={item.body} />
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
});

export const CompactionRow = memo(function CompactionRow(props: {
  item: CompactionItem;
}): JSX.Element {
  const { item } = props;
  const [open, setOpen] = useState(false);
  const label =
    item.tokensBefore === undefined
      ? "Context compacted"
      : `Context compacted · from ${formatTokens(item.tokensBefore)} tokens`;
  return (
    <Trace data-kind="compaction">
      <TraceCollapsible open={open} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger className="w-full">
          <Marker variant="separator" className="w-full">
            <MarkerContent
              data-part="label"
              className="inline-flex items-center gap-1.5 font-mono text-xs"
            >
              <BookmarkIcon className="size-3.5" />
              {label}
            </MarkerContent>
          </Marker>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent data-part="compaction-body">
          <Markdown text={item.summary} />
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
});
