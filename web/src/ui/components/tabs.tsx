"use client"

import { Tabs as TabsPrimitive } from "@base-ui/react/tabs"
import * as React from "react"
import { cn } from "../../lib/utils.ts"

function Tabs({ className, ...props }: TabsPrimitive.Root.Props) {
  return <TabsPrimitive.Root data-slot="tabs" className={cn("flex flex-col gap-2", className)} {...props} />
}

function TabsList({ className, ...props }: TabsPrimitive.List.Props) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      className={cn("inline-flex w-fit items-center gap-1 rounded-md bg-muted p-1 text-muted-foreground", className)}
      {...props}
    />
  )
}

function TabsTrigger({
  className,
  onMouseDown,
  ...props
}: TabsPrimitive.Tab.Props) {
  return (
    <TabsPrimitive.Tab
      data-slot="tabs-trigger"
      className={cn(
        "inline-flex items-center justify-center rounded-sm px-3 py-1 text-sm whitespace-nowrap transition-colors focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50 data-active:bg-background data-active:text-foreground",
        className,
      )}
      onMouseDown={(event) => {
        // Radix switched tabs on a primary-button mousedown; the base registry switches on click.
        // Replay a primary mousedown as a click so a mousedown alone still selects the tab.
        if (event.button === 0 && !event.defaultPrevented) {
          event.currentTarget.click()
        }
        onMouseDown?.(event)
      }}
      {...props}
    />
  )
}

function TabsContent({ className, ...props }: TabsPrimitive.Panel.Props) {
  return <TabsPrimitive.Panel data-slot="tabs-content" className={cn("flex-1 outline-none", className)} {...props} />
}

export { Tabs, TabsList, TabsTrigger, TabsContent }
