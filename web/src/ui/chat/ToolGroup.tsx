import { memo } from "react";
import type { JSX } from "react";
import type { ToolItem } from "../../protocol/domain.ts";
import { ToolRow } from "./ToolRow.tsx";

// Stub (DESIGN-chat.md 3.4). ToolsMode will come from useChatToggles.ts.
export type ToolsMode = "grouped" | "expanded";
export interface ToolGroupProps {
  readonly tools: readonly ToolItem[];
  readonly mode: ToolsMode;
}

export const ToolGroup = memo(function ToolGroup(props: ToolGroupProps): JSX.Element {
  return (
    <details open data-kind="tool-group">
      <summary>{props.tools.length} tool calls</summary>
      {props.tools.map((t) => (
        <ToolRow key={t.id} tool={t} />
      ))}
    </details>
  );
});
