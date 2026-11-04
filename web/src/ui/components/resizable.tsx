import type { ReactElement, ReactNode } from "react";

type PartProps = { id?: string; children?: ReactNode; [key: string]: unknown };

// Stub: renders children only. The implementation wraps react-resizable-panels (DESIGN.md 2f).
function ResizablePanelGroup(props: PartProps): ReactElement {
  return <>{props.children}</>;
}

function ResizablePanel(props: PartProps): ReactElement {
  return <>{props.children}</>;
}

function ResizableHandle(_props: PartProps & { withHandle?: boolean }): ReactElement {
  return <></>;
}

function usePanelRef(): { current: null } {
  return { current: null };
}

export { ResizableHandle, ResizablePanel, ResizablePanelGroup, usePanelRef };
