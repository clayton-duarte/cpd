import type { Cursor, TranscriptItem, Window } from "../protocol/domain.ts";

export interface TranscriptState {
  readonly items: readonly TranscriptItem[];
  readonly hasEarlier: boolean;
  readonly cursor: Cursor | null;
}

export const emptyTranscript: TranscriptState = { items: [], hasEarlier: false, cursor: null };

export function setTail(_window: Window): TranscriptState {
  throw new Error("not implemented");
}
export function prependEarlier(_state: TranscriptState, _window: Window): TranscriptState {
  throw new Error("not implemented");
}
export function appendItems(_state: TranscriptState, _items: readonly TranscriptItem[]): TranscriptState {
  throw new Error("not implemented");
}
export function updateItems(_state: TranscriptState, _items: readonly TranscriptItem[]): TranscriptState {
  throw new Error("not implemented");
}
export function applyDelta(
  _state: TranscriptState,
  _d: { itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string },
): TranscriptState {
  throw new Error("not implemented");
}
