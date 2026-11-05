import { Component } from "react";
import type { ReactNode } from "react";
import type { CrashReport } from "../electron/health.ts";
import { Alert, AlertDescription, AlertTitle } from "./components/alert.tsx";
import { Button } from "./components/button.tsx";

export interface ErrorBoundaryProps {
  name: string;
  onCrash?: (r: CrashReport) => void;
  fallback?: (ctx: { name: string; error: Error; retry(): void }) => ReactNode;
  children?: ReactNode;
}

interface State { error: Error | null }

export class ErrorBoundary extends Component<ErrorBoundaryProps, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: unknown): State {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  override componentDidCatch(error: unknown): void {
    const e = error instanceof Error ? error : new Error(String(error));
    this.props.onCrash?.({ source: "error-boundary", error: e.message, ...(e.stack === undefined ? {} : { stack: e.stack }) });
  }

  retry = (): void => { this.setState({ error: null }); };

  override render(): ReactNode {
    const { error } = this.state;
    if (error === null) return this.props.children;
    const { name, fallback } = this.props;
    if (fallback !== undefined) return fallback({ name, error, retry: this.retry });
    return (
      <Alert variant="destructive" role="alert">
        <AlertTitle>{name}</AlertTitle>
        <AlertDescription>{error.message}</AlertDescription>
        <Button type="button" variant="outline" size="sm" onClick={this.retry}>Retry</Button>
      </Alert>
    );
  }
}
