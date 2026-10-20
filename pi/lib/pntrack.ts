// Stub for the tracker of commands.md §15 "Every plannotator session is tracked"; the logic is not written yet.

/** The key plannotator keeps its process-wide review registry under, on `globalThis`. */
export const REGISTRY_KEY = "__plannotatorPiReviewRegistry_v1";

export type Tracker = {
  /** At session_start: closes as `lost` the open rows the registry lacks. */
  start(): Promise<void>;
  /** Mirrors the registry into `cpd review open` / `close`, running cpd only when it differs from the last view. */
  poll(): Promise<void>;
};

export type TrackerOptions = {
  /** `CPD_SESSION`; undefined: the adapter is not active and nothing runs. */
  session: string | undefined;
  /** Where the registry lives (`globalThis`). */
  store: Record<string, unknown>;
  /** Runs `cpd <argv>` and returns its stdout. */
  cpd: (argv: string[]) => Promise<string>;
  /** Reports a refusal (a registry of the wrong shape), once. */
  report: (message: string) => void;
};

export function createTracker(_options: TrackerOptions): Tracker {
  throw new Error("not implemented");
}
