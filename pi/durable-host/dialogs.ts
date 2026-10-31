import type * as held from "./held.ts";
import type { ToClient } from "./protocol.ts";
import type { QuestionParams, QuestionnaireResult } from "../ask-user-question/tool/types.ts";

// Contract §6.15 `dialogs.ts`: one held request per dialog, broadcast through the port, `stateChanged` on every
// request that appears or resolves. The types are real; the bodies are P1.6's, not this Job.

export type DialogPort = {
  held: held.HeldApi;
  broadcast(msg: { type: "ui_request" | "ui_resolved" | "notice" | "status" } & ToClient): void;
  stateChanged(): void;
};

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

export function createDialogs(port: DialogPort): Dialogs {
  throw new Error("not implemented");
}