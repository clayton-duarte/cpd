import { cva, type VariantProps } from "class-variance-authority";
import { mergeProps } from "@base-ui/react/merge-props";
import { useRender } from "@base-ui/react/use-render";
import * as React from "react";
import { cn } from "../../lib/utils.ts";

function Sidebar({
  collapsible = "none",
  className,
  children,
  ...props
}: React.ComponentProps<"div"> & {
  collapsible?: "offcanvas" | "icon" | "none";
}): React.ReactElement {
  return (
    <div
      data-slot="sidebar"
      data-collapsible={collapsible}
      className={cn(
        "bg-sidebar text-sidebar-foreground flex h-full w-full flex-col",
        className,
      )}
      {...props}
    >
      {children}
    </div>
  );
}

function SidebarContent({
  className,
  ...props
}: React.ComponentProps<"div">): React.ReactElement {
  return (
    <div
      data-slot="sidebar-content"
      data-sidebar="content"
      className={cn(
        "flex min-h-0 flex-1 flex-col gap-2 overflow-auto",
        className,
      )}
      {...props}
    />
  );
}

function SidebarGroup({
  className,
  ...props
}: React.ComponentProps<"div">): React.ReactElement {
  return (
    <div
      data-slot="sidebar-group"
      data-sidebar="group"
      className={cn("relative flex w-full min-w-0 flex-col p-2", className)}
      {...props}
    />
  );
}

type SidebarGroupLabelProps = Omit<
  useRender.ComponentProps<"div">,
  "className"
> & {
  className?: string;
};

function SidebarGroupLabel({
  className,
  render,
  ...props
}: SidebarGroupLabelProps): React.ReactElement {
  return useRender({
    defaultTagName: "div",
    props: mergeProps<"div">(
      {
        className: cn(
          "text-sidebar-foreground/70 ring-sidebar-ring flex h-8 shrink-0 items-center rounded-md px-2 text-xs font-medium outline-hidden transition-[margin,opacity] duration-200 ease-linear focus-visible:ring-2 [&>svg]:size-4 [&>svg]:shrink-0",
          className,
        ),
      },
      props,
    ),
    render,
    state: {
      slot: "sidebar-group-label",
      sidebar: "group-label",
    },
  });
}

type SidebarGroupActionProps = Omit<
  useRender.ComponentProps<"button">,
  "className"
> & {
  className?: string;
};

function SidebarGroupAction({
  className,
  render,
  ...props
}: SidebarGroupActionProps): React.ReactElement {
  return useRender({
    defaultTagName: "button",
    props: mergeProps<"button">(
      {
        className: cn(
          "text-sidebar-foreground ring-sidebar-ring hover:bg-sidebar-accent hover:text-sidebar-accent-foreground absolute top-3.5 right-3 flex aspect-square w-5 items-center justify-center rounded-md p-0 outline-hidden transition-transform focus-visible:ring-2 [&>svg]:size-4 [&>svg]:shrink-0",
          className,
        ),
      },
      props,
    ),
    render,
    state: {
      slot: "sidebar-group-action",
      sidebar: "group-action",
    },
  });
}

function SidebarGroupContent({
  className,
  ...props
}: React.ComponentProps<"div">): React.ReactElement {
  return (
    <div
      data-slot="sidebar-group-content"
      data-sidebar="group-content"
      className={cn("w-full text-sm", className)}
      {...props}
    />
  );
}

function SidebarMenu({
  className,
  ...props
}: React.ComponentProps<"ul">): React.ReactElement {
  return (
    <ul
      data-slot="sidebar-menu"
      data-sidebar="menu"
      className={cn("flex w-full min-w-0 flex-col gap-1", className)}
      {...props}
    />
  );
}

function SidebarMenuItem({
  className,
  ...props
}: React.ComponentProps<"li">): React.ReactElement {
  return (
    <li
      data-slot="sidebar-menu-item"
      data-sidebar="menu-item"
      className={cn("group/menu-item relative", className)}
      {...props}
    />
  );
}

const sidebarMenuButtonVariants = cva(
  "peer/menu-button flex w-full items-center gap-2 overflow-hidden rounded-md p-2 text-left text-sm outline-hidden ring-sidebar-ring transition-[width,height,padding] hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 active:bg-sidebar-accent active:text-sidebar-accent-foreground disabled:pointer-events-none disabled:opacity-50 aria-disabled:pointer-events-none aria-disabled:opacity-50 data-[active=true]:bg-sidebar-accent data-[active=true]:font-medium data-[active=true]:text-sidebar-accent-foreground [&>span:last-child]:truncate [&>svg]:size-4 [&>svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
        outline:
          "bg-background shadow-[0_0_0_1px_hsl(var(--sidebar-border))] hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
      },
      size: {
        default: "h-8 text-sm",
        sm: "h-7 text-xs",
        lg: "h-12 text-sm",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

type SidebarMenuButtonProps = Omit<
  useRender.ComponentProps<"button">,
  "className"
> & {
  className?: string;
  isActive?: boolean;
} & VariantProps<typeof sidebarMenuButtonVariants>;

function SidebarMenuButton({
  isActive = false,
  variant = "default",
  size = "default",
  className,
  render,
  ...props
}: SidebarMenuButtonProps): React.ReactElement {
  return useRender({
    defaultTagName: "button",
    props: mergeProps<"button">(
      {
        ...({ "data-size": size, "data-active": isActive } as object),
        className: cn(sidebarMenuButtonVariants({ variant, size }), className),
      },
      props,
    ),
    render,
    state: {
      slot: "sidebar-menu-button",
      sidebar: "menu-button",
      size,
      active: isActive,
    },
  });
}

type SidebarMenuActionProps = Omit<
  useRender.ComponentProps<"button">,
  "className"
> & {
  className?: string;
};

function SidebarMenuAction({
  className,
  render,
  ...props
}: SidebarMenuActionProps): React.ReactElement {
  return useRender({
    defaultTagName: "button",
    props: mergeProps<"button">(
      {
        className: cn(
          "text-sidebar-foreground ring-sidebar-ring hover:bg-sidebar-accent hover:text-sidebar-accent-foreground absolute top-1.5 right-1.5 flex aspect-square w-5 items-center justify-center rounded-md p-0 outline-hidden transition-transform focus-visible:ring-2 [&>svg]:size-4 [&>svg]:shrink-0",
          className,
        ),
      },
      props,
    ),
    render,
    state: {
      slot: "sidebar-menu-action",
      sidebar: "menu-action",
    },
  });
}

function SidebarMenuBadge({
  className,
  ...props
}: React.ComponentProps<"div">): React.ReactElement {
  return (
    <div
      data-slot="sidebar-menu-badge"
      data-sidebar="menu-badge"
      className={cn(
        "text-sidebar-foreground pointer-events-none absolute right-1 flex h-5 min-w-5 items-center justify-center rounded-md px-1 text-xs font-medium tabular-nums select-none",
        className,
      )}
      {...props}
    />
  );
}

function SidebarMenuSub({
  className,
  ...props
}: React.ComponentProps<"ul">): React.ReactElement {
  return (
    <ul
      data-slot="sidebar-menu-sub"
      data-sidebar="menu-sub"
      className={cn(
        "flex min-w-0 flex-col gap-0.5 pl-4 py-0.5",
        className,
      )}
      {...props}
    />
  );
}

function SidebarMenuSubItem({
  className,
  ...props
}: React.ComponentProps<"li">): React.ReactElement {
  return (
    <li
      data-slot="sidebar-menu-sub-item"
      data-sidebar="menu-sub-item"
      className={cn("group/menu-sub-item relative", className)}
      {...props}
    />
  );
}

type SidebarMenuSubButtonProps = Omit<
  useRender.ComponentProps<"button">,
  "className"
> & {
  className?: string;
  size?: "sm" | "md";
  isActive?: boolean;
};

function SidebarMenuSubButton({
  size = "md",
  isActive = false,
  className,
  render,
  ...props
}: SidebarMenuSubButtonProps): React.ReactElement {
  return useRender({
    defaultTagName: "button",
    props: mergeProps<"button">(
      {
        ...({ "data-size": size, "data-active": isActive } as object),
        className: cn(
          "text-sidebar-foreground ring-sidebar-ring hover:bg-sidebar-accent hover:text-sidebar-accent-foreground active:bg-sidebar-accent active:text-sidebar-accent-foreground flex h-7 w-full min-w-0 items-center gap-2 overflow-hidden rounded-md px-2 text-left outline-hidden focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-50 aria-disabled:pointer-events-none aria-disabled:opacity-50 data-[active=true]:bg-sidebar-accent data-[active=true]:text-sidebar-accent-foreground [&>span:last-child]:truncate [&>svg]:size-4 [&>svg]:shrink-0",
          size === "sm" && "text-xs",
          size === "md" && "text-sm",
          className,
        ),
      },
      props,
    ),
    render,
    state: {
      slot: "sidebar-menu-sub-button",
      sidebar: "menu-sub-button",
      size,
      active: isActive,
    },
  });
}

export {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
};
