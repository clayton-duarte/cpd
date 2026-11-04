import type { JSX } from "react";
import type { ToolStatus } from "../../protocol/domain.ts";

const fill: Readonly<Record<ToolStatus, string>> = {
  done: "bg-status-done",
  failed: "bg-status-blocked",
  running: "bg-status-active",
};

export function ToolTickStrip(props: { readonly statuses: readonly ToolStatus[] }): JSX.Element {
  const failed = props.statuses.filter((s) => s === "failed").length;
  const label = `${props.statuses.length} steps${failed > 0 ? `, ${failed} failed` : ""}`;
  return (
    <span data-part="tick-strip" role="img" aria-label={label} className="flex gap-0.5">
      {props.statuses.map((s, i) => (
        <span key={i} data-part="tick" data-status={s} className={`block h-2.5 w-0.5 rounded-[1px] ${fill[s]}`} />
      ))}
    </span>
  );
}
