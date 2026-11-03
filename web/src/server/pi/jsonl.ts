import type { Dialog, TranscriptItem } from "../../protocol/domain.ts";

export interface LinePosition {
  readonly line?: number;
  readonly offset: number;
}
export interface JsonlEntry {
  readonly type: string;
  readonly id?: string;
  readonly timestamp?: string;
  readonly [k: string]: unknown;
}
export function parseLine(_text: string, _at: LinePosition): JsonlEntry {
  throw new Error("not implemented");
}

export interface JsonlFacts {
  cwd?: string;
  sessionId?: string;
  name?: string;
  model?: string;
  thinking?: string;
  lastStopReason?: string;
  lastRole?: "user" | "assistant" | "toolResult";
}
export interface PushResult {
  added: TranscriptItem[];
  updated: TranscriptItem[];
  dialogsOpened: Dialog[];
  dialogsResolved: string[];
  factsChanged: boolean;
}
export interface PiNormalizer {
  push(text: string, at: LinePosition): PushResult;
  readonly facts: JsonlFacts;
}
export function createNormalizer(): PiNormalizer {
  throw new Error("not implemented");
}
export function normalizeText(_text: string): { items: TranscriptItem[]; facts: JsonlFacts; openDialogs: Dialog[] } {
  throw new Error("not implemented");
}
export function countItems(_entry: JsonlEntry): number {
  throw new Error("not implemented");
}
