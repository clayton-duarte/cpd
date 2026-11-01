// The typed mock of docs/research/web-client-api.md §4 — a dev-time stand-in for the decided
// API (hs-d780). Contract: token-carrying request intake that refuses a token-less request
// (§4.2, hs-d781); `ui_response` answering first-wins with `ui_unknown` for a late answer
// (§4.3, hs-d775); a typed POST action table (§4.4).
//
// Stub: every method throws `not implemented` until the impl Job fills it in — this is what
// makes the mock tests RED.
import type { SubmitResult, UiResponse } from "./types";
import type { TypedPost, TypedPostMap } from "./actions";

/** The token the client attaches to every request (§4.2, A4). */
export interface TokenCarrier {
  token: string;
}

/** A token-carrying host `submit` (§1.1, protocol.ts:29; the token is attached in transport). */
export type SubmitFrame = SubmitInput & { token: string };

/** The host `submit` a client sends; the token is optional in the input so a token-less request is representable. */
export interface SubmitInput {
  type: "submit";
  req: string;
  text: string;
  when_busy?: "followUp" | "steer" | "reject";
  request_id?: string;
  token?: string;
}

/** The outcome of answering a held `ui_request` (durable-leads 4.4): the sender's req is echoed. A late answer throws `ui_unknown`. */
export interface UiAnswerResult {
  req: string;
  data: object;
}

/**
 * The one typed API surface (§4.1) both the mock and the later real transport implement.
 */
export interface CpdApi {
  /** Host submit; refused (thrown) without the token (§4.2). */
  submit(frame: SubmitInput): SubmitResult;
  /** Answer a held `ui_request`: the first answer wins; a late one answers `ui_unknown` (§4.3). */
  uiResponse(frame: UiResponse): UiAnswerResult;
  /** A typed POST action carried to the mock; refused (thrown) without the token (§4.4). */
  post<A extends TypedPost>(action: A, carrier: TokenCarrier): TypedPostMap[A["action"]];
}

/** Builds the in-process mock implementing `CpdApi` (STUB). */
export function createMockApi(): CpdApi {
  // The mock holds every `ui_request` it has seen until it is answered; a second answer on a
  // held id is late (durable-leads 4.4 rules 2-3).
  const answered = new Set<string>();

  return {
    submit(frame: SubmitInput): SubmitResult {
      requireToken(frame.token);
      return { submission_id: `submission-${frame.req}`, status: "queued" };
    },

    uiResponse(frame: UiResponse): UiAnswerResult {
      if (answered.has(frame.id)) {
        throw new Error("ui_unknown: ui_request is no longer held");
      }
      answered.add(frame.id);
      return { req: frame.req, data: {} };
    },

    post<A extends TypedPost>(action: A, carrier: TokenCarrier): TypedPostMap[A["action"]] {
      requireToken(carrier.token);
      return postResult(action) as TypedPostMap[A["action"]];
    },
  };
}

/** Dispatches a typed POST to its mock handler, returning the action's result (\u00a74.4). */
function postResult(action: TypedPost): TypedPostMap[TypedPost["action"]] {
  switch (action.action) {
    case "submit":
      return { submission_id: "submission-post", status: "queued" };
    case "ui_response":
      return { req: action.payload.req, data: {} };
    case "history":
      return { entries: [] };
  }
}

/** The token gate: every request must carry the token (hs-d781, A4). */
function requireToken(token: string | undefined): asserts token is string {
  if (token === undefined || token.length === 0) {
    throw new Error("missing token: every request must carry the token");
  }
}
