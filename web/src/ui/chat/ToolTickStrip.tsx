import type { JSX } from "react";
import type { ToolStatus } from "../../protocol/domain.ts";

// Stub: the implementation Worker fills this in (DESIGN-chat.md 3.5).
export function ToolTickStrip(_props: { readonly statuses: readonly ToolStatus[] }): JSX.Element {
  return <span data-part="tick-strip" />;
}
