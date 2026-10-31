import { LIMITS } from "./protocol.ts";

/** Longest unix socket path in bytes: `sun_path[108]` minus the NUL (`man 7 unix`; contract §6.14). */
export const SOCKET_PATH_MAX = 107;
export type LineOut = { lines: string[]; overLimit?: string };
export class LineSplitter {
  readonly limit: number;
  constructor(limit: number = LIMITS.clientLine) {
    this.limit = limit;
  }
  push(chunk: Buffer | string): LineOut {
    void chunk;
    throw new Error("not implemented");
  }
}
export type DirStat = { exists: false } | { exists: true; isDir: boolean; uid: number; mode: number };
export type DirVerdict = "create" | "chmod" | "ok" | "refuse";
export function dirVerdict(stat: DirStat, uid: number): DirVerdict {
  void stat; void uid;
  throw new Error("not implemented");
}
export type Probe = "absent" | "refused" | "connected" | { error: string };
export type StaleVerdict = "listen" | "remove" | "host_running" | "fail";
export function staleVerdict(probe: Probe): StaleVerdict {
  void probe;
  throw new Error("not implemented");
}
