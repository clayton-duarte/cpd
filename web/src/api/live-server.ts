// The dev live-adapter /__cpd/ route handler (web/README.md "`web/src/api/live-server.ts`",
// hs-d819). Framework-free and dependency-injected: every external boundary is a `LiveDeps`
// function, so no test spawns cpd, opens a socket or starts a server.
//
// Stub: `createLiveHandler` throws `not implemented` until the impl Job fills it in — this is
// what makes the live-server tests RED.
import type { IncomingMessage, ServerResponse } from "node:http";

/** One Session the picker offers. */
export interface LiveSession {
  id: string;
  role: "lead" | "manager";
  runner: "pi" | "durable";
  title?: string;
  state?: string;
}

/** Every external boundary is injected; the handler itself touches nothing real. */
export interface LiveDeps {
  /** The lead and manager Sessions, from `cpd session list --role lead --role manager`. */
  list(): Promise<readonly LiveSession[]>;
  /** A pi Session's session file, as its full JSONL text; undefined when there is none (a durable Session, or an unknown id). */
  readPi(id: string): Promise<string | undefined>;
  /** A durable Session's transcript: the full stdout of `cpd session tail <id>`. Called only for runner "durable". */
  tail(id: string): Promise<string>;
  /** One send: `cpd session send <id> <text> --unblocks <reason>`. The only mutation there is. */
  send(id: string, text: string, unblocks: string): Promise<void>;
  /** A pi Session's file mtime in ms, undefined when absent: the frames stream's change signal. */
  stat(id: string): Promise<number | undefined>;
}

/** Handles one request; resolves true when it matched a /__cpd/ route (the caller stops), false to pass through. */
export type LiveHandler = (req: IncomingMessage, res: ServerResponse) => Promise<boolean>;

/** Builds the injected-route handler (STUB). */
export function createLiveHandler(deps: LiveDeps): LiveHandler {
  void deps;
  throw new Error("not implemented");
}
