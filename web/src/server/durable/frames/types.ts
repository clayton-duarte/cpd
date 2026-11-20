import type { Dialog, TranscriptItem } from "../../../protocol/domain.ts";

export interface DurableFacts {
  model?: string;
  thinking?: string | null;
  busy: boolean;
  compacting: boolean;
  costUsd: number;
  lastContextTokens?: number;
  queued: { id: number; mode: string }[];
  cwd?: string;
}
export interface DurableTranscript {
  readonly items: TranscriptItem[];
  readonly facts: DurableFacts;
  readonly before?: number;
  readonly openDialog?: Dialog | undefined;
}

export interface DialectOutput {
  append: TranscriptItem[];
  update: TranscriptItem[];
  deltas: { itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string }[];
  busy?: boolean;
  compacting?: boolean;
  queued?: { id: number; mode: string }[];
  agent?: { model?: string; thinking?: string | null };
  replaced?: boolean;
}
export interface Gap {
  readonly after: number;
  readonly upTo: number;
}
/** A `snapshot` (protocol 1) or `view` (protocol 2) frame; epoch and seq are the protocol 2 stream cursor. */
export type BaseFrame = { snapshot: unknown } | { view: unknown; epoch?: string; seq?: number };
/** An `events` (protocol 1) or `ops` (protocol 2) frame. */
export type IncrementFrame = { events: unknown[] } | { ops: unknown[]; epoch?: string; seq?: number };
export interface Dialect {
  applyBase(frame: BaseFrame): { transcript: DurableTranscript };
  applyIncrement(frame: IncrementFrame): DialectOutput;
  applyHistory(frame: { entries: readonly unknown[]; next?: string }): void;
  readonly gap: Gap | undefined;
  readonly transcript: DurableTranscript;
}
