"use client"

import { Collapsible as CollapsiblePrimitive } from "@base-ui/react/collapsible"
import * as React from "react"
import { cn } from "../../lib/utils.ts"

if (import.meta.env.MODE === "test") {
  (globalThis as { BASE_UI_ANIMATIONS_DISABLED?: boolean }).BASE_UI_ANIMATIONS_DISABLED = true
}

function Collapsible({
  open,
  ...props
}: CollapsiblePrimitive.Root.Props) {
  return (
    <CollapsiblePrimitive.Root
      data-slot="collapsible"
      data-state={open === undefined ? undefined : open ? "open" : "closed"}
      open={open}
      {...props}
    />
  )
}

function CollapsibleTrigger({
  ...props
}: CollapsiblePrimitive.Trigger.Props) {
  return (
    <CollapsiblePrimitive.Trigger
      data-slot="collapsible-trigger"
      {...props}
    />
  )
}

function CollapsibleContent({
  className,
  ...props
}: CollapsiblePrimitive.Panel.Props) {
  return (
    <CollapsiblePrimitive.Panel
      data-slot="collapsible-content"
      className={cn(
        "overflow-hidden data-open:animate-collapsible-down data-closed:animate-collapsible-up",
        className,
      )}
      {...props}
    />
  )
}

export { Collapsible, CollapsibleTrigger, CollapsibleContent }
