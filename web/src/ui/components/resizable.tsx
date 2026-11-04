import * as React from "react";
import * as ResizablePrimitive from "react-resizable-panels";
import { cn } from "../../lib/utils.ts";

function ResizablePanelGroup({
  className,
  id,
  defaultLayout: defaultLayoutProp,
  onLayoutChanged: onLayoutChangedProp,
  ...props
}: ResizablePrimitive.GroupProps): React.ReactElement {
  const persisted = ResizablePrimitive.useDefaultLayout({
    id: id ?? "cpd-resizable",
  });
  return (
    <ResizablePrimitive.Group
      id={id}
      defaultLayout={defaultLayoutProp ?? (id ? persisted.defaultLayout : undefined)}
      onLayoutChanged={(layout, meta) => {
        if (id) persisted.onLayoutChanged(layout, meta);
        onLayoutChangedProp?.(layout, meta);
      }}
      data-slot="resizable-panel-group"
      className={cn(
        "flex h-full w-full aria-[orientation=vertical]:flex-col",
        className,
      )}
      {...props}
    />
  );
}

function ResizablePanel({
  className,
  ...props
}: ResizablePrimitive.PanelProps): React.ReactElement {
  return (
    <ResizablePrimitive.Panel
      data-slot="resizable-panel"
      className={cn("min-h-0 min-w-0", className)}
      {...props}
    />
  );
}

function ResizableHandle({
  withHandle,
  className,
  ...props
}: ResizablePrimitive.SeparatorProps & {
  withHandle?: boolean;
}): React.ReactElement {
  return (
    <ResizablePrimitive.Separator
      data-slot="resizable-handle"
      className={cn(
        "bg-border focus-visible:ring-ring relative flex w-px items-center justify-center after:absolute after:inset-y-0 after:left-1/2 after:w-2 after:-translate-x-1/2 focus-visible:ring-1 focus-visible:outline-hidden aria-[orientation=horizontal]:h-px aria-[orientation=horizontal]:w-full aria-[orientation=horizontal]:after:left-0 aria-[orientation=horizontal]:after:h-2 aria-[orientation=horizontal]:after:w-full aria-[orientation=horizontal]:after:translate-x-0 aria-[orientation=horizontal]:after:-translate-y-1/2 [&[aria-orientation=horizontal]>div]:rotate-90",
        className,
      )}
      {...props}
    >
      {withHandle ? (
        <div className="bg-border z-10 flex h-6 w-1 shrink-0 rounded-lg" />
      ) : null}
    </ResizablePrimitive.Separator>
  );
}

const usePanelRef = ResizablePrimitive.usePanelRef;

export { ResizableHandle, ResizablePanel, ResizablePanelGroup, usePanelRef };
