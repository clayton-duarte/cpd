import type { Cursor, Dialog, Window } from "../../protocol/domain.ts";
import type { PiNormalizer } from "./jsonl.ts";

export interface FileStat {
  readonly size: number;
  readonly identity: string;
}
export interface FileBoundary {
  stat(path: string): Promise<FileStat>;
  read(path: string, start: number, end: number): Promise<Uint8Array>;
}
export const CHUNK_BYTES = 65_536;
export function cursorOf(_offset: number): Cursor {
  throw new Error("not implemented");
}
export function offsetOf(_cursor: Cursor): number {
  throw new Error("not implemented");
}
export interface TailWindow {
  window: Window;
  startOffset: number;
  endOffset: number;
  identity: string;
  normalizer: PiNormalizer;
  openDialogs: Dialog[];
}
export function readTail(_path: string, _fs: FileBoundary, _size?: number): Promise<TailWindow> {
  return Promise.reject(new Error("not implemented"));
}
export function readEarlier(
  _path: string,
  _fs: FileBoundary,
  _before: Cursor,
  _size?: number,
): Promise<{ window: Window; startOffset: number }> {
  return Promise.reject(new Error("not implemented"));
}
