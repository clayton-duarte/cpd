import { type VariantProps } from "class-variance-authority";
import { Toggle as TogglePrimitive } from "@base-ui/react/toggle";
import { ToggleGroup as ToggleGroupPrimitive } from "@base-ui/react/toggle-group";
import * as React from "react";
import { cn } from "../../lib/utils.ts";
import { toggleVariants } from "./toggle.tsx";

const ToggleGroupContext = React.createContext<
  VariantProps<typeof toggleVariants> & {
    spacing?: number;
    orientation?: "horizontal" | "vertical";
  }
>({
  size: "default",
  variant: "default",
  spacing: 0,
  orientation: "horizontal",
});

type ToggleGroupProps = Omit<ToggleGroupPrimitive.Props, "multiple"> &
  VariantProps<typeof toggleVariants> & {
    spacing?: number;
    orientation?: "horizontal" | "vertical";
    type?: "single" | "multiple";
  };

function ToggleGroup({
  className,
  variant,
  size,
  spacing = 0,
  orientation = "horizontal",
  type,
  children,
  ...props
}: ToggleGroupProps): React.ReactElement {
  return (
    <ToggleGroupPrimitive
      data-slot="toggle-group"
      data-variant={variant}
      data-size={size}
      data-spacing={spacing}
      data-orientation={orientation}
      orientation={orientation}
      multiple={type === "multiple"}
      style={{ "--gap": spacing } as React.CSSProperties}
      className={cn(
        "group/toggle-group flex w-fit items-center gap-[--spacing(var(--gap))] rounded-md data-[orientation=vertical]:flex-col data-[orientation=vertical]:items-stretch data-[spacing=default]:data-[variant=outline]:shadow-xs",
        className,
      )}
      {...props}
    >
      <ToggleGroupContext.Provider
        value={{ variant, size, spacing, orientation }}
      >
        {children}
      </ToggleGroupContext.Provider>
    </ToggleGroupPrimitive>
  );
}

function ToggleGroupItem({
  className,
  children,
  variant,
  size,
  ...props
}: TogglePrimitive.Props &
  VariantProps<typeof toggleVariants>): React.ReactElement {
  const context = React.useContext(ToggleGroupContext);

  return (
    <TogglePrimitive
      data-slot="toggle-group-item"
      data-variant={context.variant || variant}
      data-size={context.size || size}
      data-spacing={context.spacing}
      data-orientation={context.orientation}
      className={cn(
        toggleVariants({
          variant: context.variant || variant,
          size: context.size || size,
        }),
        "w-auto min-w-0 shrink-0 px-3 focus:z-10 focus-visible:z-10",
        "data-[spacing=0]:rounded-none data-[spacing=0]:shadow-none",
        "data-[orientation=horizontal]:data-[spacing=0]:first:rounded-l-md data-[orientation=horizontal]:data-[spacing=0]:last:rounded-r-md data-[orientation=horizontal]:data-[spacing=0]:data-[variant=outline]:border-l-0 data-[orientation=horizontal]:data-[spacing=0]:data-[variant=outline]:first:border-l",
        "data-[orientation=vertical]:w-full data-[orientation=vertical]:justify-between data-[orientation=vertical]:data-[spacing=0]:first:rounded-t-md data-[orientation=vertical]:data-[spacing=0]:last:rounded-b-md data-[orientation=vertical]:data-[spacing=0]:data-[variant=outline]:border-t-0 data-[orientation=vertical]:data-[spacing=0]:data-[variant=outline]:first:border-t",
        className,
      )}
      {...props}
    >
      {children}
    </TogglePrimitive>
  );
}

export { ToggleGroup, ToggleGroupItem };
