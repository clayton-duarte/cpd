import { memo, useState } from "react";
import type { JSX } from "react";
import type { ToolItem } from "../../protocol/domain.ts";
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
    <details
      data-kind="tool-group"
      data-tool-count={tools.length}
      data-status={agg}
      open={open}
      onToggle={(e) => setOpen(e.currentTarget.open)}
      className="group pl-3 shadow-[inset_2px_0_0_var(--input)]"
    >
      <summary
        data-part="group-header"
        className="flex h-[26px] cursor-pointer list-none items-center gap-2.5 rounded-[5px] px-2 text-[12px] hover:bg-row-hover [&::-webkit-details-marker]:hidden"
      >
        <span data-part="caret" aria-hidden="true" className="font-mono text-[10px] text-muted-foreground group-open:rotate-90">▸</span>
        <span data-part="glyph" aria-hidden="true" data-tone={tone} data-glyph={tone} className={glyphClass[tone]} />
        <span data-part="name" className="font-mono text-fg-strong">{first === undefined ? "" : toolName(first)}</span>
        <span data-part="summary" className="min-w-0 truncate text-foreground">
          {`${first === undefined ? "" : toolDetail(first)} + ${n} more ${n === 1 ? "step" : "steps"}`}
        </span>
        <ToolTickStrip statuses={tools.map((t) => t.status)} />
        <span className="flex-1" />
        <span data-part="totals" className="font-mono text-meta text-muted-foreground">{totals}</span>
        <span data-part="group-count" className="sr-only">{`${tools.length} tool calls`}</span>
      </summary>
      <div data-part="group-rows" className="flex flex-col gap-0.5">
        {tools.map((t) => (
          <ToolRow key={t.id} tool={t} nested />
        ))}
      </div>
    </details>
  );
}, sameProps);
