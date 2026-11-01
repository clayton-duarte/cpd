// The transcript parsers and the snapshot mapping of the dev live adapter (web/README.md "The
// TypeScript contract", hs-d819). A real cpd Session renders into the existing Chat through one
// fold `snapshot` frame: a pi Session is parsed from its session file's JSONL, a durable Session
// from the plain lines of `cpd session tail` (docs/contract.md 6.20).
//
// Stub: every function throws `not implemented` until the impl Job fills it in — this is what
// makes the transcript tests RED.
import type { SnapshotFrame } from "./fold/fold";

/** One rendered transcript message of a real Session. */
export interface TranscriptMessage {
  id: string;
  role: "user" | "assistant" | "system" | "tool";
  /** The tool name of a `tool` message (the tail grammar's `<toolName>`). */
  name?: string;
  text: string;
  /** A failed tool result — the tail grammar's ` (error)` suffix. */
  error?: true;
}

/** Parses a pi session file's JSONL text into messages (pi parse rules, web/README.md). */
export function parsePiSession(jsonl: string): TranscriptMessage[] {
  void jsonl;
  throw new Error("not implemented");
}

/** Parses `cpd session tail` output in the grammar of contract §6.20 (tail parse rules). */
export function parseTailOutput(output: string): TranscriptMessage[] {
  void output;
  throw new Error("not implemented");
}

/** Maps messages onto one fold `snapshot` frame (fold rule 3): `snapshot: { messages }`. Defaults: epoch "live", seq 1. */
export function toSnapshotFrame(
  messages: readonly TranscriptMessage[],
  epoch = "live",
  seq = 1,
): SnapshotFrame {
  void messages;
  void epoch;
  void seq;
  throw new Error("not implemented");
}
