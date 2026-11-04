import { memo, useState } from "react";
import type { JSX } from "react";
import type { ToolItem } from "../../protocol/domain.ts";
import { Trace, TraceCollapsible, TraceCollapsibleContent, TraceCollapsibleTrigger } from "../components/trace.tsx";
import { glyphClass } from "../dango-status.ts";
import { ToolRow, duration, statusTone, toolDetail, toolName } from "./ToolRow.tsx";
import { ToolTickStrip } from "./ToolTickStrip.tsx";

export type ToolsMode = "grouped" | "expanded";
export interface ToolGroupProps {
  readonly tools: readonly ToolItem[];
  readonly mode: ToolsMode;
}

const sameProps = (a: ToolGroupProps, b: ToolGroupProps): boolean =>
  a.mode === b.mode && a.tools.length === b.tools.length && a.tools.every((t, i) => t === b.tools[i]);

export const ToolGroup = memo(function ToolGroup(props: ToolGroupProps): JSX.Element {
  const { tools } = props;
  const [open, setOpen] = useState(props.mode === "expanded");
  const failed = tools.filter((t) => t.status === "failed").length;
  const running = tools.some((t) => t.status === "running");
  const timed = tools.filter((t) => typeof t.result?.durationMs === "number");
  const total = timed.reduce((n, t) => n + (t.result?.durationMs ?? 0), 0);
  const agg = failed > 0 ? "failed" : running ? "running" : "done";
  const tone = statusTone(agg);
  const first = tools[0];
  const n = tools.length - 1;
  const totals = `${running ? "running" : "done"}${timed.length > 0 && !running ? ` in ${duration(total)}` : ""}${failed > 0 ? ` · ${failed} failed` : ""}`;
  return (
    <Trace data-kind="tool-group" data-tool-count={tools.length} data-status={agg} data-state={open ? "open" : "closed"}>
      <TraceCollapsible open={open} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger data-part="group-header" className="w-full">
          <span className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2.5 gap-y-0.5 text-[12px]">
            <span data-part="glyph" aria-hidden="true" data-tone={tone} data-glyph={tone} className={glyphClass[tone]} />
            <span data-part="name" className="font-mono text-fg-strong">{first === undefined ? "" : toolName(first)}</span>
            <span data-part="summary" className="min-w-0 break-words text-foreground">
              {`${first === undefined ? "" : toolDetail(first)} + ${n} more ${n === 1 ? "step" : "steps"}`}
            </span>
            <ToolTickStrip statuses={tools.map((t) => t.status)} />
            <span data-part="totals" className="font-mono text-meta text-muted-foreground">{totals}</span>
            <span data-part="group-count" className="sr-only">{`${tools.length} tool calls`}</span>
          </span>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent forceMount className="mt-1 mb-0 ml-0 border-l-0 pr-0 pl-4">
          <div data-part="group-rows" className="flex flex-col gap-1">
            {tools.map((t) => (
              <ToolRow key={t.id} tool={t} defaultOpen={props.mode === "expanded"} />
            ))}
          </div>
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}, sameProps);
