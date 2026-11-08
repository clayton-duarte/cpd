import type { ReactElement } from "react";
import type { CrashReport } from "../electron/health.ts";
import { App } from "./App.tsx";
import type { AppProps } from "./App.tsx";
import { Alert, AlertDescription, AlertTitle } from "./components/alert.tsx";
import { Button } from "./components/button.tsx";
import { ErrorBoundary } from "./ErrorBoundary.tsx";

export type RootProps = AppProps & {
  onCrash?: (r: CrashReport) => void;
  onHealthy?: () => void;
  errors?: () => number;
  reload?: () => void;
};

const noop = (): void => {};
const reloadWindow = (): void => window.location.reload();

export function Root(props: RootProps): ReactElement {
  const { onCrash, onHealthy = noop, reload = reloadWindow, ...appProps } = props;
  // web/CONTRACT.md §15.2: the App reports health:ok itself, only after the first cockpit.snapshot.
  return (
    <ErrorBoundary
      name="App"
      {...(onCrash === undefined ? {} : { onCrash })}
      fallback={({ name, error }) => (
        <div className="fixed inset-0 grid place-items-center p-4">
          <Alert variant="destructive" role="alert" className="max-w-md">
            <AlertTitle>{name}</AlertTitle>
            <AlertDescription>{error.message}</AlertDescription>
            <Button type="button" variant="outline" size="sm" onClick={reload}>Reload</Button>
          </Alert>
        </div>
      )}
    >
      <App {...appProps} onHealthy={onHealthy} {...(onCrash === undefined ? {} : { onCrash })} />
    </ErrorBoundary>
  );
}
