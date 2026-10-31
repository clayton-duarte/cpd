import { StringDecoder } from "node:string_decoder";
import { LIMITS } from "./protocol.ts";

/** Longest unix socket path in bytes: `sun_path[108]` minus the NUL (`man 7 unix`; contract §6.14). */
export const SOCKET_PATH_MAX = 107;
export type LineOut = { lines: string[]; overLimit?: string };
export class LineSplitter {
  readonly limit: number;
  constructor(limit: number = LIMITS.clientLine) {
    this.limit = limit;
  }
  #decoder = new StringDecoder("utf8");
  #pending = "";
  #discarding = false;
  push(chunk: Buffer | string): LineOut {
    let text = this.#pending + (typeof chunk === "string" ? chunk : this.#decoder.write(chunk));
    this.#pending = "";
    const out: LineOut = { lines: [] };
    for (;;) {
      const nl = text.indexOf("\n");
      if (this.#discarding) {
        if (nl < 0) return out;
        this.#discarding = false;
        text = text.slice(nl + 1);
        continue;
      }
      const line = nl < 0 ? text : text.slice(0, nl);
      if (Buffer.byteLength(line) > this.limit) {
        out.overLimit = Buffer.from(line).subarray(0, this.limit + 1).toString();
        if (nl < 0) this.#discarding = true;
        else text = text.slice(nl + 1);
        if (nl < 0) return out;
        continue;
      }
      if (nl < 0) {
        this.#pending = text;
        return out;
      }
      out.lines.push(line);
      text = text.slice(nl + 1);
    }
  }
}
export type DirStat = { exists: false } | { exists: true; isDir: boolean; uid: number; mode: number };
export type DirVerdict = "create" | "chmod" | "ok" | "refuse";
export function dirVerdict(stat: DirStat, uid: number): DirVerdict {
  if (!stat.exists) return "create";
  if (!stat.isDir || stat.uid !== uid) return "refuse";
  return (stat.mode & 0o777) === 0o700 ? "ok" : "chmod";
}
export type Probe = "absent" | "refused" | "connected" | { error: string };
export type StaleVerdict = "listen" | "remove" | "host_running" | "fail";
export function staleVerdict(probe: Probe): StaleVerdict {
  if (probe === "absent") return "listen";
  if (probe === "refused") return "remove";
  if (probe === "connected") return "host_running";
  return "fail";
}
