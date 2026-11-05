import { CircleCheckIcon, CircleXIcon } from "lucide-react";
import { memo, useEffect, useState } from "react";
import type { JSX } from "react";
import type { ToolItem } from "../../protocol/domain.ts";
import { Spinner } from "../components/spinner.tsx";
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
import {
  ToolRow,
  duration,
  statusTone,
  toolDetail,
  toolName,
} from "./ToolRow.tsx";
import { ToolTickStrip } from "./ToolTickStrip.tsx";

export type ToolsMode = "grouped" | "expanded";
export interface ToolGroupProps {
  readonly tools: readonly ToolItem[];
  readonly mode: ToolsMode;
}

const sameProps = (a: ToolGroupProps, b: ToolGroupProps): boolean =>
  a.mode === b.mode &&
  a.tools.length === b.tools.length &&
  a.tools.every((t, i) => t === b.tools[i]);

export const ToolGroup = memo(function ToolGroup(
  props: ToolGroupProps,
): JSX.Element {
  const { tools } = props;
  const [open, setOpen] = useState<boolean | undefined>(undefined);
  useEffect(() => {
    setOpen(undefined);
  }, [props.mode]);

  const failed = tools.filter((t) => t.status === "failed").length;
  const running = tools.some((t) => t.status === "running");
  const isOpen = open ?? (running || props.mode === "expanded");
  const timed = tools.filter((t) => typeof t.result?.durationMs === "number");
  const total = timed.reduce((n, t) => n + (t.result?.durationMs ?? 0), 0);
  const agg = failed > 0 ? "failed" : running ? "running" : "done";
  const tone = statusTone(agg);
  const first = tools[0];
  const n = tools.length - 1;
  const totals = `${running ? "running" : "done"}${timed.length > 0 && !running ? ` in ${duration(total)}` : ""}${failed > 0 ? ` · ${failed} failed` : ""}`;
  const traceStatus =
    agg === "running" ? "running" : agg === "failed" ? "error" : undefined;

  return (
    <Trace
      data-kind="tool-group"
      data-tool-count={tools.length}
      data-status={agg}
    >
      <TraceCollapsible open={isOpen} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger data-part="group-header">
          <TraceLine status={traceStatus} className="text-xs">
            <TraceIcon data-part="glyph" data-tone={tone} data-glyph={tone}>
              {agg === "running" ? (
                <Spinner />
              ) : agg === "failed" ? (
                <CircleXIcon className="text-destructive" />
              ) : (
                <CircleCheckIcon />
              )}
            </TraceIcon>
            <span
              data-part="name"
              className="shrink-0 font-mono text-fg-strong"
            >
              {first === undefined ? "" : toolName(first)}
            </span>
            <TraceTitle data-part="summary" className="text-foreground">
              {`${first === undefined ? "" : toolDetail(first)} + ${n} more ${n === 1 ? "step" : "steps"}`}
            </TraceTitle>
            <ToolTickStrip statuses={tools.map((t) => t.status)} />
            <TraceDescription data-part="totals" className="font-mono">
              {totals}
            </TraceDescription>
            <span data-part="group-count" className="sr-only">
              {`${tools.length} tool calls`}
            </span>
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent>
          <Trace data-part="group-rows">
            {tools.map((t) => (
              <ToolRow key={t.id} tool={t} nested />
            ))}
          </Trace>
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}, sameProps);
