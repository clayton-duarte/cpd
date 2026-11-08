import { PreviewCard as PreviewCardPrimitive } from "@base-ui/react/preview-card";
import * as React from "react";
import { cn } from "../../lib/utils.ts";

function HoverCard({
  ...props
}: PreviewCardPrimitive.Root.Props): React.ReactElement {
  return <PreviewCardPrimitive.Root data-slot="hover-card" {...props} />;
}

type HoverCardTriggerProps = PreviewCardPrimitive.Trigger.Props & {
  asChild?: boolean;
};

function HoverCardTrigger({
  asChild = false,
  render,
  children,
  ...props
}: HoverCardTriggerProps): React.ReactElement {
  const renderProp =
    asChild && React.isValidElement(children) ? children : render;
  return (
    <PreviewCardPrimitive.Trigger
      data-slot="hover-card-trigger"
      render={renderProp}
      {...props}
    >
      {asChild ? undefined : children}
    </PreviewCardPrimitive.Trigger>
  );
}

function HoverCardContent({
  className,
  align = "center",
  alignOffset = 0,
  side = "bottom",
  sideOffset = 4,
  ...props
}: PreviewCardPrimitive.Popup.Props &
  Pick<
    PreviewCardPrimitive.Positioner.Props,
    "align" | "alignOffset" | "side" | "sideOffset"
  >): React.ReactElement {
  return (
    <PreviewCardPrimitive.Portal data-slot="hover-card-portal">
      <PreviewCardPrimitive.Positioner
        align={align}
        alignOffset={alignOffset}
        side={side}
        sideOffset={sideOffset}
        className="isolate z-50"
      >
        <PreviewCardPrimitive.Popup
          data-slot="hover-card-content"
          className={cn(
            "z-50 w-64 origin-(--transform-origin) rounded-md border bg-popover p-4 text-popover-foreground shadow-md outline-hidden data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95",
            className,
          )}
          {...props}
        />
      </PreviewCardPrimitive.Positioner>
    </PreviewCardPrimitive.Portal>
  );
}

export { HoverCard, HoverCardContent, HoverCardTrigger };
