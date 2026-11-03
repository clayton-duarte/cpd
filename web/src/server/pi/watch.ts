import { WebError } from "../../protocol/domain.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { PiNormalizer, PushResult } from "./jsonl.ts";
import { CHUNK_BYTES } from "./window.ts";
import type { FileBoundary } from "./window.ts";

export interface WatchBoundary {
  watch(path: string, onChange: () => void): Disposable;
}
export type WatchEvent =
  | { kind: "append"; result: PushResult; endOffset: number }
  | { kind: "reset"; reason: "truncated" | "replaced" };

const NL = 10;
const decoder = new TextDecoder();

function mergeInto(into: PushResult, r: PushResult): void {
  for (const item of r.added) into.added.push(item);
  for (const u of r.updated) {
    const inAdded = into.added.findIndex((x) => x.id === u.id);
    if (inAdded >= 0) {
      into.added[inAdded] = u;
      continue;
    }
    const inUpdated = into.updated.findIndex((x) => x.id === u.id);
    if (inUpdated >= 0) into.updated[inUpdated] = u;
    else into.updated.push(u);
  }
  into.dialogsOpened.push(...r.dialogsOpened);
  into.dialogsResolved.push(...r.dialogsResolved);
  into.factsChanged = into.factsChanged || r.factsChanged;
}

export function watchSession(deps: {
  path: string;
  fs: FileBoundary;
  watch: WatchBoundary;
  normalizer: PiNormalizer;
  offset: number;
  identity: string;
  onEvent: (e: WatchEvent) => void;
  onError: (e: WebError) => void;
}): Disposable {
  const { path, fs, normalizer } = deps;
  let offset = deps.offset;
  let disposed = false;
  let inert = false;
  let reading = false;
  let again = false;

  const stopped = (): boolean => disposed || inert;

  async function step(): Promise<void> {
    const stat = await fs.stat(path);
    if (stopped()) return;
    if (stat.identity !== deps.identity || stat.size < offset) {
      inert = true;
      deps.onEvent({ kind: "reset", reason: stat.identity !== deps.identity ? "replaced" : "truncated" });
      return;
    }
    if (stat.size === offset) return;
    let buf = new Uint8Array(0);
    for (let pos = offset; pos < stat.size; pos += CHUNK_BYTES) {
      const piece = await fs.read(path, pos, Math.min(stat.size, pos + CHUNK_BYTES));
      if (stopped()) return;
      const next = new Uint8Array(buf.length + piece.length);
      next.set(buf);
      next.set(piece, buf.length);
      buf = next;
    }
    const last = buf.lastIndexOf(NL);
    if (last < 0) return;
    const result: PushResult = { added: [], updated: [], dialogsOpened: [], dialogsResolved: [], factsChanged: false };
    const errors: WebError[] = [];
    let lineStart = 0;
    while (lineStart <= last) {
      const nl = buf.indexOf(NL, lineStart);
      const text = decoder.decode(buf.subarray(lineStart, nl));
      if (text.trim() !== "") {
        try {
          mergeInto(result, normalizer.push(text, { offset: offset + lineStart }));
        } catch (e) {
          if (!(e instanceof WebError)) throw e;
          errors.push(e);
        }
      }
      lineStart = nl + 1;
    }
    offset += last + 1;
    deps.onEvent({ kind: "append", result, endOffset: offset });
    for (const e of errors) deps.onError(e);
  }

  async function drain(): Promise<void> {
    reading = true;
    try {
      do {
        again = false;
        try {
          await step();
        } catch (e) {
          if (stopped()) return;
          deps.onError(
            e instanceof WebError
              ? e
              : new WebError("session_file_unreadable", `cannot read ${path}: ${e instanceof Error ? e.message : String(e)}`, { path }),
          );
        }
      } while (again && !stopped());
    } finally {
      reading = false;
    }
  }

  const handle = deps.watch.watch(path, () => {
    if (stopped()) return;
    if (reading) again = true;
    else void drain();
  });

  return {
    dispose(): void {
      if (disposed) return;
      disposed = true;
      handle.dispose();
    },
  };
}
