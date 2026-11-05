import * as React from "react";
import { cn } from "../../lib/utils.ts";

function Input({ className, type, ...props }: React.ComponentProps<"input">): React.ReactElement {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "h-9 w-full min-w-0 rounded-md border border-input bg-transparent px-3 py-1 text-sm text-foreground outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/50 aria-invalid:border-destructive aria-invalid:ring-destructive/30 disabled:pointer-events-none disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Input };
