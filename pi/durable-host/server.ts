import { encode, LIMITS, negotiate, PROTOCOL_VERSIONS, parseLine, ProtocolError, type AgentState, type ToClient, type ToHost, type HostState } from "./protocol.ts";
import { type Frame, type Ring } from "./ring.ts";

export type Connection = { write(line: string): void; buffered(): number; close(): void };
export type Timers = { after(ms: number, fn: () => void): () => void };

/** One held UI request as the server sees it: what the replay at attach sends (§6.15, `held.ts`'s list, oldest first). */
export type HeldUI = {
  id: string;
  method: "select" | "confirm" | "input" | "editor" | "questionnaire";
  purpose: "ask_user_question" | "plan_review" | "secret_approve" | "host_approve" | "external_approve" | "mcp_login" | "other";
  created_at: number;
  expires_at?: number;
  fields: object;
};

export type UiAnswer = { value?: string; confirmed?: boolean; answer?: object; cancelled?: boolean };

export type ServerDeps = {
  session: string;
  role: "lead" | "manager";
  cwd: string;
  hostVersion: string;
  piDurable: string;
  epoch: string;
  ring: Ring;
  submit(input: { text: string; whenBusy: "followUp" | "steer" | "reject"; requestId?: string; images?: readonly { mimeType: string; data: string }[] }): Promise<{ submissionId: number; status: string }>;
  /** Contract §6.17: the five conversation commands; each port settles one reply or throws a `ProtocolError`. */
  compact(instructions?: string): Promise<{ task_id: number }>;
  configure(change: { model?: string; thinkingLevel?: string | null }): Promise<{ agent: AgentState }>;
  models(): Promise<readonly { provider: string; model_id: string; name: string; thinking_levels: readonly string[]; context_window: number }[]>;
  fork(at: number, ownership?: "lead" | "manager"): Promise<{ conversation_id: string }>;
  history(before?: string, limit?: number): Promise<{ entries: readonly unknown[]; next?: string }>;
  abort(opts: { background: boolean }): Promise<void>;
  abortSubmission(id: number): Promise<"aborted" | "already_placed" | "settled" | "not_found">;
  /** Rebuilds and installs the adapter extension (contract §6.18); the answer is one `result { reloaded: true }`. */
  reload(): Promise<{ reloaded: boolean }>;
  hostState(): HostState;
  /** The process's held map, oldest first: what an attaching client is replayed (§6.15 rule 4). */
  held(): readonly HeldUI[];
  /** Answers one held request: rejects with `ProtocolError('ui_unknown', ...)` when the id is not held (§6.15 rule 3). */
  answerUi(id: string, value: UiAnswer): Promise<{ outcome: "answered" | "cancelled" }>;
  timers: Timers;
  clientId(): string;
};

export type Server = {
  connect(conn: Connection): { receive(line: string): void; closed(): void };
  frame(f: Frame): void;
  stateChanged(): void;
  /** Writes a live `ui_request`, `ui_resolved`, `notice` or `status` to every attached client; a connection that has not attached gets nothing (§6.15 rule 5). */
  broadcast(msg: { type: "ui_request" | "ui_resolved" | "notice" | "status" } & ToClient): void;
  shutdown(reason: "shutdown" | "restart" | "storage_failed"): void;
  clients(): number;
};

export type { ToClient, ToHost };

type State = {
  conn: Connection;
  greeted: boolean;
  id: string;
  attached: boolean;
  closed: boolean;
  cancelHello: () => void;
};

export function createServer(deps: ServerDeps): Server {
  const states = new Set<State>();
  let down = false;

  const drop = (st: State): void => {
    if (st.closed) return;
    st.closed = true;
    st.cancelHello();
    states.delete(st);
    st.conn.close();
  };
  const send = (st: State, msg: ToClient): void => {
    if (st.closed) return;
    st.conn.write(encode(msg));
    if (st.conn.buffered() > LIMITS.slowClient) {
      st.conn.write(encode({ type: "bye", reason: "slow_client", reconnect: true }));
      drop(st);
    }
  };
  const fail = (st: State, code: Extract<ToClient, { type: "error" }>["code"], message: string, req?: string, details?: object): void =>
    send(st, { type: "error", ...(req === undefined ? {} : { req }), code, message, ...(details === undefined ? {} : { details }) });
  const failWith = (st: State, e: unknown, req: string): void => {
    if (e instanceof ProtocolError) fail(st, e.code, e.message, req, e.details);
    else fail(st, "internal", e instanceof Error ? e.message : String(e), req);
  };
  const hostState = (): ToClient => {
    const { state, name, agent, held_ui } = deps.hostState();
    return { type: "host_state", state, ...(name === undefined ? {} : { name }), clients: count(), held_ui, ...(agent === undefined ? {} : { agent }) };
  };
  const count = (): number => [...states].filter((s) => s.attached).length;
  const broadcast: Server["broadcast"] = (msg) => {
    for (const st of [...states]) if (st.attached) send(st, msg);
  };

  const run = (st: State, req: string, work: () => Promise<object>): void => {
    work().then(
      (data) => send(st, { type: "result", req, data }),
      (e) => failWith(st, e, req),
    );
  };

  const handle = (st: State, msg: ToHost): void => {
    switch (msg.type) {
      case "hello":
        fail(st, "bad_args", "hello already received");
        return;
      case "ping":
        send(st, { type: "pong", ...(msg.req === undefined ? {} : { req: msg.req }) });
        return;
      case "attach": {
        if (msg.conversation !== undefined && msg.conversation !== "root") {
          fail(st, "unsupported", "only the root conversation is supported", msg.req);
          return;
        }
        const a = deps.ring.attach(msg.cursor);
        st.attached = true;
        send(st, { type: "attached", req: msg.req, mode: a.mode, epoch: deps.epoch, from_seq: a.fromSeq, head_seq: a.headSeq, client_id: st.id });
        for (const f of a.frames) send(st, f);
        for (const h of deps.held()) {
          send(st, { ...h.fields, type: "ui_request", id: h.id, method: h.method, purpose: h.purpose, created_at: h.created_at, ...(h.expires_at === undefined ? {} : { expires_at: h.expires_at }), replayed: true } as ToClient);
        }
        send(st, hostState());
        return;
      }
      case "detach":
        send(st, { type: "result", req: msg.req, data: {} });
        send(st, { type: "bye", reason: "detached", reconnect: false });
        drop(st);
        return;
      default:
        if (!st.attached) {
          fail(st, "not_attached", "attach first", msg.req);
          return;
        }
        if (msg.type === "submit") {
          run(st, msg.req, async () => {
            const imgs = msg.images === undefined ? {} : { images: msg.images.map((i) => ({ mimeType: i.mime_type, data: i.data })) };
            const r = await deps.submit({ text: msg.text, whenBusy: msg.when_busy, ...(msg.request_id === undefined ? {} : { requestId: msg.request_id }), ...imgs });
            return { submission_id: r.submissionId, status: r.status };
          });
        } else if (msg.type === "compact") {
          run(st, msg.req, async () => ({ task_id: (await deps.compact(msg.instructions)).task_id }));
        } else if (msg.type === "configure") {
          run(st, msg.req, async () => {
            const change: { model?: string; thinkingLevel?: string | null } = {};
            if (msg.model !== undefined) change.model = msg.model;
            if (msg.thinking_level !== undefined) change.thinkingLevel = msg.thinking_level;
            return { agent: (await deps.configure(change)).agent };
          });
        } else if (msg.type === "list_models") {
          run(st, msg.req, async () => ({ models: await deps.models() }));
        } else if (msg.type === "fork") {
          run(st, msg.req, async () => ({ conversation_id: (await deps.fork(msg.at, msg.ownership)).conversation_id }));
        } else if (msg.type === "history") {
          run(st, msg.req, async () => {
            const r = await deps.history(msg.before, msg.limit);
            return { entries: r.entries, ...(r.next === undefined ? {} : { next: r.next }) };
          });
        } else if (msg.type === "abort") {
          run(st, msg.req, async () => {
            await deps.abort({ background: msg.background });
            return {};
          });
        } else if (msg.type === "abort_submission") {
          run(st, msg.req, async () => ({ outcome: await deps.abortSubmission(msg.submission_id) }));
        } else if (msg.type === "reload") {
          run(st, msg.req, () => deps.reload());
        } else if (msg.type === "ui_response") {
          const done = (): void => send(st, { type: "result", req: msg.req, data: {} });
          deps
            .answerUi(msg.id, { value: msg.value, confirmed: msg.confirmed, answer: msg.answer, cancelled: msg.cancelled })
            .then(
              (r) => {
                done();
                broadcast({ type: "ui_resolved", id: msg.id, outcome: r.outcome, by: st.id });
              },
              (e) => failWith(st, e, msg.req),
            );
        }
    }
  };

  return {
    connect(conn) {
      const st: State = { conn, greeted: false, id: "", attached: false, closed: false, cancelHello: () => {} };
      if (down) {
        fail(st, "shutting_down", "host is shutting down");
        drop(st);
        return { receive() {}, closed() {} };
      }
      states.add(st);
      st.cancelHello = deps.timers.after(LIMITS.helloMs, () => {
        if (st.greeted) return;
        fail(st, "hello_timeout", "no hello received");
        drop(st);
      });
      return {
        receive(line) {
          if (st.closed) return;
          if (line.length > LIMITS.clientLine) {
            fail(st, "frame_too_large", "line exceeds the client line limit");
            drop(st);
            return;
          }
          const p = parseLine(line);
          if (!p.ok) {
            fail(st, p.error.code, p.error.message, p.error.req);
            return;
          }
          const msg = p.msg;
          if (!st.greeted) {
            if (msg.type === "ping") return handle(st, msg);
            if (msg.type !== "hello") {
              fail(st, "hello_required", "send hello first", "req" in msg ? msg.req : undefined);
              return;
            }
            const v = negotiate(msg);
            if (v === undefined) {
              fail(st, "protocol_mismatch", "no common protocol version", undefined, { host: [...PROTOCOL_VERSIONS], client: [msg.min_protocol, msg.protocol] });
              drop(st);
              return;
            }
            st.greeted = true;
            st.id = deps.clientId();
            st.cancelHello();
            send(st, {
              type: "welcome",
              protocol: v,
              host_version: deps.hostVersion,
              pi_durable: deps.piDurable,
              session: deps.session,
              role: deps.role,
              epoch: deps.epoch,
              client_id: st.id,
              cwd: deps.cwd,
              limits: { client_line: LIMITS.clientLine },
            });
            return;
          }
          handle(st, msg);
        },
        closed() {
          drop(st);
        },
      };
    },
    frame(f) {
      for (const st of [...states]) if (st.attached) send(st, f);
    },
    stateChanged() {
      for (const st of [...states]) if (st.attached) send(st, hostState());
    },
    broadcast,
    shutdown(reason) {
      down = true;
      for (const st of [...states]) {
        send(st, { type: "bye", reason, reconnect: true });
        drop(st);
      }
    },
    clients: count,
  };
}
