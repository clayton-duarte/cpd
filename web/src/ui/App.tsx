import type { ReactElement } from "react";
import type { Scheduler, Transport } from "../protocol/runtime.ts";

// STUB (red-test Job): the implementation Job replaces this body.
export interface FileReaderBoundary {
  readBase64(file: { readonly name: string; readonly type: string; readonly size: number }): Promise<string>;
  readText(file: { readonly name: string; readonly type: string; readonly size: number }): Promise<string>;
}
export interface AppProps { transport: Transport; scheduler: Scheduler; reader: FileReaderBoundary; newId?: () => string }
export function App(_props: AppProps): ReactElement {
  throw new Error("not implemented");
}
