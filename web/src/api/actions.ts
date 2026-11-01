// The typed POST action builders (docs/research/web-client-api.md §4.4): every client action
// is a typed POST carried to the mock. The builders themselves are plain data constructors
// (the mock's action handlers are the stubs that throw — see mock.ts).
import type { HistoryResult, SubmitResult, UiResponse } from "./types";

const WHEN_BUSY = ["followUp", "steer", "reject"] as const;
export type WhenBusy = (typeof WHEN_BUSY)[number];

/** The `submit` host command as a POST (§4.4; protocol.ts:29). */
export interface SubmitAction {
  action: "submit";
  payload: { text: string; when_busy?: WhenBusy; request_id?: string };
}

/** Answering a held `ui_request` as a POST (§4.3, hs-d775). */
export interface UiResponseAction {
  action: "ui_response";
  payload: UiResponse;
}

/** The `history` P1.8 result as a POST (§1.2). */
export interface HistoryAction {
  action: "history";
  payload: { before?: string; limit?: number };
}

/** Every typed POST the client offers (§4.4). */
export type TypedPost = SubmitAction | UiResponseAction | HistoryAction;

/** A typed POST action name maps to the result its mock handler returns. */
export interface TypedPostMap {
  submit: SubmitResult;
  ui_response: { req: string; data: object };
  history: HistoryResult;
}

/** Builds the `submit` POST. */
export function submitPost(text: string, extra?: Omit<SubmitAction["payload"], "text">): SubmitAction {
  return { action: "submit", payload: { text, ...extra } };
}

/** Builds the `ui_response` POST. */
export function uiResponsePost(payload: UiResponse): UiResponseAction {
  return { action: "ui_response", payload };
}

/** Builds the `history` POST. */
export function historyPost(payload: HistoryAction["payload"] = {}): HistoryAction {
  return { action: "history", payload };
}
