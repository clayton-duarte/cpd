import type { ReactElement } from "react";
import type { CrashReport } from "../electron/health.ts";
import { App } from "./App.tsx";
import type { AppProps } from "./App.tsx";

export type RootProps = AppProps & {
  onCrash?: (r: CrashReport) => void;
  onHealthy?: () => void;
  errors?: () => number;
  reload?: () => void;
};

// Stub: renders App only.
export function Root(props: RootProps): ReactElement {
  const { transport, scheduler, reader, newId } = props;
  return <App transport={transport} scheduler={scheduler} reader={reader} {...(newId === undefined ? {} : { newId })} />;
}
