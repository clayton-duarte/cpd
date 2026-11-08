// The app's structured Event sink (air-d81, docs/commands.md §11): every watchdog detection and
// recovery is one JSON line — kind, text, data — the same shape install.ts reports a broken
// installation with. The composition root appends the lines to a file under $CPD_HOME/run.

import type { WatchdogEvent } from "./watchdog.ts";

export interface EventReporterDeps {
  /** Where the lines land (the app's event sink). */
  file: string;
  /** Append one line; the real one is `node:fs/promises`'s `appendFile`. */
  append(file: string, line: string): Promise<void>;
  /** The one clock; defaults to `Date.now`. */
  now?(): number;
}

export interface EventReporter {
  report(event: WatchdogEvent): Promise<void>;
}

/** One structured cpd Event as a JSON line, ready to append and read back with `cpd events`-style readers. */
export function createEventReporter(deps: EventReporterDeps): EventReporter {
  const now = deps.now ?? Date.now;
  let seq = 0;
  return {
    async report(event: WatchdogEvent): Promise<void> {
      const line = JSON.stringify({
        seq: ++seq,
        at: new Date(now()).toISOString(),
        kind: event.kind,
        text: event.text,
        data: event.data,
      });
      await deps.append(deps.file, `${line}\n`);
    },
  };
}
