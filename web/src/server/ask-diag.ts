import { appendFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";

/** Default ask-diag sink: appends one JSON line to `<cpdHome>/run/ask-diag.log`. Swallows its own errors: diagnostics may not break answers. */
export function createAskDiagSink(cpdHome: string): (record: Readonly<Record<string, unknown>>) => void {
  const file = join(cpdHome, "run", "ask-diag.log");
  return (record) => {
    try {
      mkdirSync(dirname(file), { recursive: true });
      appendFileSync(file, `${JSON.stringify(record)}\n`);
    } catch {
      // deliberate exception to fail-loud: see above
    }
  };
}
