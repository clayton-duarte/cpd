import type * as held from "./held.ts";
import type { ToClient } from "./protocol.ts";
import type { QuestionParams, QuestionnaireResult } from "../ask-user-question/tool/types.ts";

// Contract §6.15 `dialogs.ts`: one held request per dialog, broadcast through the port, `stateChanged` on every
// request that appears or resolves. The dialog maps the request's settlement to the method's pi shape: a settled
// `undefined` is the method's cancel (`undefined`, `false`, `{ cancelled: true }` with empty answers).

export type DialogPort = {
  held: held.HeldApi;
  broadcast(msg: { type: "ui_request" | "ui_resolved" | "notice" | "status" } & ToClient): void;
  stateChanged(): void;
};

/** A live host-to-client message: one of the §6.15 one-shots, never `replayed`. */
type Live = { type: "ui_request" | "ui_resolved" | "notice" | "status" } & ToClient;

export type DialogOptions = { purpose?: held.Held["purpose"]; ms?: number; signal?: AbortSignal; toolCallId?: string };

export type Dialogs = {
  select(title: string, options: readonly string[], opts?: DialogOptions): Promise<string | undefined>;
  confirm(title: string, message: string, opts?: DialogOptions): Promise<boolean | undefined>;
  input(title: string, placeholder?: string, opts?: DialogOptions): Promise<string | undefined>;
  editor(title: string, prefill?: string, opts?: DialogOptions): Promise<string | undefined>;
  questionnaire(questions: QuestionParams["questions"], opts?: DialogOptions): Promise<QuestionnaireResult>;
  notice(level: "info" | "warning" | "error", message: string): void;
  status(key: string, text: string | null): void;
};

/** A request this dialog owns: the held entry's id and the outcome its settlement recorded, for the host-settled broadcast. */
type Owned = { id: string; outcome(): held.Outcome | undefined };

export function createDialogs(port: DialogPort): Dialogs {
  /** One held request per dialog (4.4 rule 1): created, broadcast as `ui_request`, the state re-reported. */
  const run = (method: held.Held["method"], fields: object, opts: DialogOptions | undefined): Owned & { promise: Promise<object | undefined> } => {
    const purpose = opts?.purpose ?? "other";
    let outcome: held.Outcome | undefined;
    const { id, promise } = port.held.create(method, fields, purpose, {
      ...(opts?.ms === undefined ? {} : { ms: opts.ms }),
      ...(opts?.signal === undefined ? {} : { signal: opts.signal }),
      ...(opts?.toolCallId === undefined ? {} : { toolCallId: opts.toolCallId }),
      onResolve: (o) => {
        outcome = o;
      },
    });
    const created_at = Date.now();
    port.broadcast({
      ...fields,
      type: "ui_request",
      id,
      method,
      purpose,
      created_at,
      ...(opts?.ms === undefined ? {} : { expires_at: created_at + opts.ms }),
      ...(opts?.toolCallId === undefined ? {} : { tool_call_id: opts.toolCallId }),
      // STUB (hostfix-tests-a1): wrong on purpose, the flag is not derived from the purpose yet; it follows a questionnaire header.
      ...(JSON.stringify(fields).includes('"header":"Approval"') ? { approval: true } : {}),
    } as Live);
    port.stateChanged();
    return { id, promise, outcome: () => outcome };
  };

  /** What the dialog does when the request settles: the host-settled kinds are broadcast as `ui_resolved` (G52), every resolution re-reports the state. */
  const settled = async (owned: Owned & { promise: Promise<object | undefined> }): Promise<object | undefined> => {
    const value = await owned.promise;
    port.stateChanged();
    const outcome = owned.outcome();
    if (outcome === "timeout" || outcome === "superseded") {
      port.broadcast({ type: "ui_resolved", id: owned.id, outcome, by: "host" });
    }
    return value;
  };

  return {
    select: async (title, options, opts) => {
      const value = await settled(run("select", { title, options }, opts));
      return value === undefined ? undefined : String(value);
    },
    confirm: async (title, message, opts) => {
      const value = await settled(run("confirm", { title, message }, opts));
      return value === undefined ? false : Boolean(value);
    },
    input: async (title, placeholder, opts) => {
      const value = await settled(run("input", { title, ...(placeholder === undefined ? {} : { placeholder }) }, opts));
      return value === undefined ? undefined : String(value);
    },
    editor: async (title, prefill, opts) => {
      const value = await settled(run("editor", { title, ...(prefill === undefined ? {} : { prefill }) }, opts));
      return value === undefined ? undefined : String(value);
    },
    questionnaire: async (questions, opts) => {
      const value = await settled(run("questionnaire", { questions }, opts));
      return (value as QuestionnaireResult | undefined) ?? { answers: [], cancelled: true };
    },
    notice: (level, message) => port.broadcast({ type: "notice", level, message }),
    status: (key, text) => port.broadcast({ type: "status", key, text }),
  };
}
