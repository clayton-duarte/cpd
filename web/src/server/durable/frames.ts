import type { Dialog, TranscriptItem } from "../../protocol/domain.ts";
import type { HostFrame } from "./client.ts";

export interface DurableFacts {
  model?: string;
  thinking?: string | null;
  costUsd: number;
  lastContextTokens?: number;
  queued: { id: number; mode: string }[];
  cwd?: string;
}
export interface DurableTranscript {
  readonly items: TranscriptItem[];
  readonly facts: DurableFacts;
}

export const SUPPORTED_PI_DURABLE = "1.3.0";
export function assertSupportedHost(_welcome: { pi_durable: string; protocol: number }): void {
  throw new Error("not implemented");
}

export interface DialectOutput {
  append: TranscriptItem[];
  update: TranscriptItem[];
  deltas: { itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string }[];
  busy?: boolean;
  queued?: { id: number; mode: string }[];
  agent?: { model?: string; thinking?: string | null };
  replaced?: boolean;
}
export interface Dialect {
  applySnapshot(frame: { snapshot: unknown }): { transcript: DurableTranscript };
  applyEvents(frame: { events: unknown[] }): DialectOutput;
  readonly transcript: DurableTranscript;
}
export function createDialect(): Dialect {
  throw new Error("not implemented");
}
export function dialogFromRequest(
  _req: Extract<HostFrame, { type: "ui_request" }>,
  _transcript: readonly TranscriptItem[],
): Dialog | { kind: "editor"; id: string } | { kind: "unsupported"; id: string; method: string; purpose: string } {
  throw new Error("not implemented");
}
