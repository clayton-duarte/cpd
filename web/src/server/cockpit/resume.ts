import { toArgv } from "../cpd-argv.ts";
import type { Cpd } from "./cli.ts";

/**
 * Server action to resume a stopped pi Session.
 * Invokes `cpd session resume <key>`.
 */
export async function resumeSession(cpd: Cpd, sessionKey: string): Promise<Record<string, unknown>> {
  return await cpd.json(toArgv({ kind: "session.resume", sessionId: sessionKey }));
}
