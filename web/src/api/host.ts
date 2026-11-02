// The host-socket client of the dev live adapter (web/README.md "`web/src/api/host.ts` — the
// socket client", hs-d819/chat-host). One connect + `hello` + `attach` per `attach()` call, the
// parsed frames out, reconnecting per the README's rule; `submit()` is the durable send (the
// audit's Verdict 4, web/README.md "Send"). The node socket is the one external boundary:
// `node:net` in production, a fake in tests. No test opens a real socket.
import type { Socket } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";

/** A frame the host sent: protocol.ts's ToClient, the subset a hello/attach-only reader receives. */
export type HostFrame =
  | { type: "welcome"; session: string; role: "lead" | "manager"; epoch: string }
  | {
      type: "attached";
      req: string;
      mode: "snapshot" | "resume";
      epoch: string;
      from_seq: number;
      head_seq: number;
    }
  | { type: "snapshot"; epoch: string; seq: number; snapshot: unknown }
  | { type: "events"; epoch: string; seq: number; events: unknown[] }
  | { type: "host_state"; state: "idle" | "working" | "waiting" | "blocked" }
  | { type: "error"; code: string; message: string }
  | { type: "bye"; reason: string; reconnect: boolean };

/** The one external boundary: the node socket (node:net in production, replaced in tests). */
export type HostConnect = (path: string) => Promise<Socket>;

/** The host-socket client: one connect + hello + attach per attach() call, frames out, reconnecting per the rule below. */
export interface HostClient {
  /**
   * Attaches to the Session's host socket and calls onFrame per received frame until the returned
   * closure closes the socket. `onUnavailable` is called once when no live host answers (a refused
   * connect), so a one-shot read can fall back to `cpd session tail` instead of retrying forever.
   */
  attach(id: string, onFrame: (frame: HostFrame) => void, onUnavailable?: () => void): () => void;
  /** One durable send: hello + attach + `submit`, settling on the host's `result` (web/README.md "Send"). */
  submit?(id: string, text: string, whenBusy?: "followUp" | "steer" | "reject"): Promise<void>;
}

/** The reconnect back-off of the README's rule: 100 ms doubling to a 1 s cap over a 30 s deadline. */
const BACKOFF_START_MS = 100;
const BACKOFF_CAP_MS = 1000;
const BACKOFF_DEADLINE_MS = 30_000;
/** The client's hello: the highest protocol it speaks and its floor (protocol 1). It sends nothing else. */
const HELLO = { type: "hello", protocol: 1, min_protocol: 1, client: "cpd web chat" } as const;
/** The frames a hello/attach-only reader accepts; a frame outside this subset is refused. */
const ALLOWED_FRAMES: ReadonlySet<string> = new Set([
  "welcome",
  "attached",
  "snapshot",
  "events",
  "host_state",
  "error",
  "bye",
]);

/** The Session's host socket path: `$CPD_HOME/run/host/<id>.sock`, `$CPD_HOME` defaulting to `~/.cpd`. */
function hostSocketPath(id: string): string {
  return join(process.env.CPD_HOME ?? join(homedir(), ".cpd"), "run", "host", `${id}.sock`);
}

export function createHostClient(connect: HostConnect): HostClient {
  return {
    attach(id, onFrame, onUnavailable) {
      const path = hostSocketPath(id);
      let closed = false;
      let cursor: { epoch: string; seq: number } | undefined;
      let socket: Socket | undefined;
      let timer: ReturnType<typeof setTimeout> | undefined;
      let attempt = 0;
      const startedAt = Date.now();

      // On any socket loss (error, EOF, close) reconnect with the back-off, giving up past the deadline.
      const scheduleReconnect = (): void => {
        if (closed) return;
        if (Date.now() - startedAt > BACKOFF_DEADLINE_MS) return;
        const delay = Math.min(BACKOFF_START_MS * 2 ** attempt, BACKOFF_CAP_MS);
        attempt += 1;
        timer = setTimeout(() => void open(), delay);
      };

      const open = async (): Promise<void> => {
        if (closed) return;
        let next: Socket;
        try {
          next = await connect(path);
        } catch {
          onUnavailable?.();
          scheduleReconnect();
          return;
        }
        if (closed) {
          next.destroy();
          return;
        }
        socket = next;
        let buffer = "";
        let sentAttach = false;
        let lost = false;

        const handleLine = (line: string): void => {
          let parsed: unknown;
          try {
            parsed = JSON.parse(line);
          } catch {
            // bad_frame: a line that is not JSON is skipped.
            return;
          }
          if (typeof parsed !== "object" || parsed === null) return;
          const frame = parsed as Record<string, unknown>;
          const type = frame["type"];
          if (typeof type !== "string" || !ALLOWED_FRAMES.has(type)) return;

          // `hello` first; once `welcome` arrives, one `attach` with the saved cursor (absent on a first attach).
          if (type === "welcome" && !sentAttach) {
            sentAttach = true;
            const attach: { type: "attach"; req: string; cursor?: { epoch: string; seq: number } } = {
              type: "attach",
              req: "r1",
            };
            if (cursor !== undefined) attach.cursor = cursor;
            next.write(`${JSON.stringify(attach)}\n`);
          }
          // The cursor is the last applied stream frame; the reconnect sends it.
          if (
            (type === "snapshot" || type === "events") &&
            typeof frame["epoch"] === "string" &&
            typeof frame["seq"] === "number"
          ) {
            cursor = { epoch: frame["epoch"], seq: frame["seq"] };
          }
          onFrame(parsed as HostFrame);
        };

        const onData = (chunk: Buffer | string): void => {
          buffer += typeof chunk === "string" ? chunk : chunk.toString("utf8");
          for (let index = buffer.indexOf("\n"); index >= 0; index = buffer.indexOf("\n")) {
            const line = buffer.slice(0, index);
            buffer = buffer.slice(index + 1);
            if (line.length > 0) handleLine(line);
          }
        };

        const onLost = (): void => {
          if (lost) return;
          lost = true;
          next.removeListener("data", onData);
          next.removeListener("error", onLost);
          next.removeListener("end", onLost);
          next.removeListener("close", onLost);
          scheduleReconnect();
        };

        next.on("data", onData);
        next.on("error", onLost);
        next.on("end", onLost);
        next.on("close", onLost);
        next.write(`${JSON.stringify(HELLO)}\n`);
      };

      void open();

      return () => {
        closed = true;
        if (timer !== undefined) clearTimeout(timer);
        socket?.destroy();
      };
    },

    submit(id, text, whenBusy = "followUp") {
      const path = hostSocketPath(id);
      return new Promise<void>((resolve, reject) => {
        let settled = false;
        let socket: Socket | undefined;
        let buffer = "";
        const finish = (error?: Error): void => {
          if (settled) return;
          settled = true;
          socket?.destroy();
          if (error === undefined) resolve();
          else reject(error);
        };
        // The submit handshake: `hello` → `welcome` → `attach` → `attached` → `submit` → `result`.
        let stage = 0;
        const handleLine = (line: string): void => {
          let parsed: unknown;
          try {
            parsed = JSON.parse(line);
          } catch {
            return;
          }
          if (typeof parsed !== "object" || parsed === null) return;
          const frame = parsed as Record<string, unknown>;
          const type = frame["type"];
          if (type === "welcome" && stage === 0) {
            stage = 1;
            socket?.write(`${JSON.stringify({ type: "attach", req: "r1" })}\n`);
          } else if (type === "attached" && stage === 1) {
            stage = 2;
            socket?.write(
              `${JSON.stringify({ type: "submit", req: "r1", text, when_busy: whenBusy })}\n`,
            );
          } else if (type === "result" && stage === 2) {
            finish();
          } else if (type === "error") {
            finish(new Error(`host error: ${String(frame["code"])}`));
          } else if (type === "bye") {
            finish(new Error("host closed the connection"));
          }
        };
        connect(path).then(
          (next) => {
            if (settled) {
              next.destroy();
              return;
            }
            socket = next;
            next.on("data", (chunk: Buffer | string) => {
              buffer += typeof chunk === "string" ? chunk : chunk.toString("utf8");
              for (let index = buffer.indexOf("\n"); index >= 0; index = buffer.indexOf("\n")) {
                const line = buffer.slice(0, index);
                buffer = buffer.slice(index + 1);
                if (line.length > 0) handleLine(line);
              }
            });
            next.on("error", () => finish(new Error("host socket error")));
            next.on("end", () => finish(new Error("host socket ended")));
            next.on("close", () => finish(new Error("host socket closed")));
            next.write(`${JSON.stringify(HELLO)}\n`);
          },
          () => finish(new Error("host socket unavailable")),
        );
      });
    },
  };
}
