import type { WebError } from "../../protocol/domain.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { PiNormalizer, PushResult } from "./jsonl.ts";
import type { FileBoundary } from "./window.ts";

export interface WatchBoundary {
  watch(path: string, onChange: () => void): Disposable;
}
export type WatchEvent =
  | { kind: "append"; result: PushResult; endOffset: number }
  | { kind: "reset"; reason: "truncated" | "replaced" };
export function watchSession(_deps: {
  path: string;
  fs: FileBoundary;
  watch: WatchBoundary;
  normalizer: PiNormalizer;
  offset: number;
  identity: string;
  onEvent: (e: WatchEvent) => void;
  onError: (e: WebError) => void;
}): Disposable {
  throw new Error("not implemented");
}
