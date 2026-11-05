import { useRef } from "react";
import type { ReactElement } from "react";
import type { CrashReport } from "../electron/health.ts";
import { App } from "./App.tsx";
import type { AppProps } from "./App.tsx";
import { Alert, AlertDescription, AlertTitle } from "./components/alert.tsx";
import { Button } from "./components/button.tsx";
import { ErrorBoundary } from "./ErrorBoundary.tsx";
import { HealthBeacon } from "./HealthBeacon.tsx";

export type RootProps = AppProps & {
  onCrash?: (r: CrashReport) => void;
  onHealthy?: () => void;
  errors?: () => number;
  reload?: () => void;
};

const noErrors = (): number => 0;
const noop = (): void => {};
const reloadWindow = (): void => window.location.reload();

export function Root(props: RootProps): ReactElement {
  const { onCrash, onHealthy = noop, errors = noErrors, reload = reloadWindow, ...appProps } = props;
  const sentinel = useRef<HTMLSpanElement>(null);
  // The container this tree is mounted in (#root), judged by what App rendered beside the sentinel.
  const root = (): HTMLElement | null => {
    const el = sentinel.current;
    const parent = el?.parentElement ?? null;
    if (parent === null) return null;
    return [...parent.children].some((c) => c !== el) ? parent : null;
  };
  return (
    <>
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
        <App {...appProps} {...(onCrash === undefined ? {} : { onCrash })} />
      </ErrorBoundary>
      <span ref={sentinel} hidden />
      <HealthBeacon root={root} errors={errors} onHealthy={onHealthy} />
    </>
  );
}
