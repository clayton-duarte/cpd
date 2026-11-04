import { WINDOW_SIZE, WebError } from "../../protocol/domain.ts";
import type { Cursor, Dialog, TranscriptItem, Window } from "../../protocol/domain.ts";
import { applyResult, countItems, createNormalizer, parseLine } from "./jsonl.ts";
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

export function cursorOf(offset: number): Cursor {
  return `pi:${offset}`;
}
export function offsetOf(cursor: Cursor): number {
  const m = /^pi:(\d+)$/.exec(cursor);
  const n = m === null ? Number.NaN : Number(m[1]);
  if (!Number.isSafeInteger(n)) throw new WebError("invalid_message", `not a pi cursor: ${cursor}`, { field: "before" });
  return n;
}
export interface TailWindow {
  window: Window;
  startOffset: number;
  endOffset: number;
  identity: string;
  normalizer: PiNormalizer;
  openDialogs: Dialog[];
}

const NL = 10;
const FIRST_READ_BYTES = 4096; // reads start small and double up to CHUNK_BYTES, so a short tail never pays for a full chunk
const decoder = new TextDecoder();

const unreadable = (path: string, e: unknown): WebError =>
  e instanceof WebError
    ? e
    : new WebError("session_file_unreadable", `cannot read ${path}: ${e instanceof Error ? e.message : String(e)}`, { path });

const concat = (a: Uint8Array, b: Uint8Array): Uint8Array => {
  const out = new Uint8Array(a.length + b.length);
  out.set(a);
  out.set(b, a.length);
  return out;
};

/** Offset just after the last "\n" in [0, size), scanning backwards in chunks; 0 when there is none. */
async function lastNewlineEnd(path: string, fs: FileBoundary, size: number): Promise<number> {
  let pos = size;
  let want = FIRST_READ_BYTES;
  while (pos > 0) {
    const start = Math.max(0, pos - want);
    want = Math.min(CHUNK_BYTES, want * 2);
    const buf = await fs.read(path, start, pos);
    const i = buf.lastIndexOf(NL);
    if (i >= 0) return start + i + 1;
    pos = start;
  }
  return 0;
}

interface Line {
  readonly offset: number;
  readonly text: string;
}

/** Whole lines ending at `end` (a line boundary), newest first, until `size` items are counted or offset 0. */
async function readLinesBackward(path: string, fs: FileBoundary, end: number, size: number): Promise<Line[]> {
  const lines: Line[] = [];
  let count = 0;
  let pos = end;
  let carry: Uint8Array = new Uint8Array(0); // incomplete leading segment (with its "\n") of the region read so far
  let want = FIRST_READ_BYTES * 2;
  while (pos > 0 && count < size) {
    const start = Math.max(0, pos - want);
    want = Math.min(CHUNK_BYTES, want * 2);
    const buf = concat(await fs.read(path, start, pos), carry);
    // buf covers [start, start + buf.length) and ends on a line boundary
    const first = buf.indexOf(NL);
    const complete = start === 0 ? 0 : first + 1; // index where the first complete line begins
    let lineEnd = buf.length; // exclusive, includes the "\n"
    while (lineEnd > complete && count < size) {
      const prev = lineEnd >= 2 ? buf.lastIndexOf(NL, lineEnd - 2) : -1;
      const lineStart = prev < complete - 1 ? complete : prev + 1;
      const text = decoder.decode(buf.subarray(lineStart, lineEnd - 1));
      if (text.trim() !== "") {
        const offset = start + lineStart;
        lines.push({ offset, text });
        count += countItems(parseLine(text, { offset }));
      } else {
        lines.push({ offset: start + lineStart, text: "" });
      }
      lineEnd = lineStart;
    }
    if (count >= size) break;
    carry = buf.subarray(0, complete);
    pos = start;
  }
  return lines;
}

function build(lines: Line[], end: number): { window: Window; startOffset: number; normalizer: PiNormalizer; openDialogs: Dialog[] } {
  const startOffset = lines.length === 0 ? end : (lines[lines.length - 1] as Line).offset;
  const normalizer = createNormalizer();
  const items: TranscriptItem[] = [];
  const open = new Map<string, Dialog>();
  for (let i = lines.length - 1; i >= 0; i--) {
    const l = lines[i] as Line;
    if (l.text !== "") applyResult(items, open, normalizer.push(l.text, { offset: l.offset }));
  }
  const hasEarlier = startOffset > 0;
  return { window: { items, hasEarlier, cursor: hasEarlier ? cursorOf(startOffset) : null }, startOffset, normalizer, openDialogs: [...open.values()] };
}

export async function readTail(path: string, fs: FileBoundary, size: number = WINDOW_SIZE): Promise<TailWindow> {
  try {
    const stat = await fs.stat(path);
    const endOffset = stat.size === 0 ? 0 : await lastNewlineEnd(path, fs, stat.size);
    const lines = endOffset === 0 ? [] : await readLinesBackward(path, fs, endOffset, size);
    return { ...build(lines, endOffset), endOffset, identity: stat.identity };
  } catch (e) {
    throw unreadable(path, e);
  }
}

export async function readEarlier(
  path: string,
  fs: FileBoundary,
  before: Cursor,
  size: number = WINDOW_SIZE,
): Promise<{ window: Window; startOffset: number }> {
  const end = offsetOf(before);
  try {
    const lines = end === 0 ? [] : await readLinesBackward(path, fs, end, size);
    const { window, startOffset } = build(lines, end);
    return { window, startOffset };
  } catch (e) {
    throw unreadable(path, e);
  }
}
