import type { JSX } from "react";
import type { OrphanResultItem, ToolItem } from "../../protocol/domain.ts";

export function ToolRow(_props: { tool: ToolItem }): JSX.Element {
  throw new Error("not implemented");
}

export function OrphanRow(_props: { item: OrphanResultItem }): JSX.Element {
  throw new Error("not implemented");
}
