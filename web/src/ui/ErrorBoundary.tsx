import type { ReactElement, ReactNode } from "react";
import type { CrashReport } from "../electron/health.ts";

export interface ErrorBoundaryProps {
  name: string;
  onCrash?: (r: CrashReport) => void;
  fallback?: (ctx: { name: string; error: Error; retry(): void }) => ReactNode;
  children?: ReactNode;
}

// Stub: renders children only (the implementation Job adds the boundary).
export function ErrorBoundary(props: ErrorBoundaryProps): ReactElement {
  return <>{props.children}</>;
}
