import type { ReactElement, ReactNode } from "react";

// Stub: the implementation Job replaces these with the shadcn context menu (Dango theme).
type Parts = { children?: ReactNode };
export function ContextMenu(props: Parts): ReactElement { return <>{props.children}</>; }
export function ContextMenuTrigger(props: Parts): ReactElement { return <>{props.children}</>; }
export function ContextMenuContent(props: Parts): ReactElement { return <>{props.children}</>; }
export function ContextMenuItem(props: Parts & { onSelect?: () => void }): ReactElement { return <>{props.children}</>; }
export function ContextMenuSeparator(): ReactElement { return <></>; }
