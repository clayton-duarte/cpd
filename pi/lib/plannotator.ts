import { randomUUID } from "node:crypto";
import { CpdError, isObject } from "./cpd.ts";
import type { PiApi } from "./pi.ts";

/** commands.md §15.5: plannotator's event API (cpd 1 trial t4), parsed at the boundary because it is not a stable contract. */

export type Respond = { status: "handled"; result: unknown } | { status: "unavailable" | "error"; error: unknown };

function parseRespond(raw: unknown): Respond {
  if (!isObject(raw)) throw new CpdError("plannotator_invalid", `plannotator answered with ${JSON.stringify(raw)}`);
  if (raw.status === "handled") return { status: "handled", result: raw.result };
  if (raw.status === "unavailable" || raw.status === "error") return { status: raw.status, error: raw.error };
  throw new CpdError("plannotator_invalid", `plannotator answered with status ${JSON.stringify(raw.status)}`);
}

const errorText = (e: unknown): string => {
  if (typeof e === "string") return e;
  if (isObject(e) && typeof e.message === "string") return e.message;
  return e === undefined ? "no error given" : JSON.stringify(e);
};

/** `pi.events.emit("plannotator:request", …)`, waiting at most `timeoutMs` for `respond`. */
export function request(pi: PiApi, action: string, payload: Record<string, unknown>, timeoutMs: number): Promise<Respond> {
  return new Promise<Respond>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new CpdError("plannotator_timeout", `plannotator did not answer ${action} within ${timeoutMs} ms (is @plannotator/pi-extension loaded?)`)),
      timeoutMs,
    );
    pi.events.emit("plannotator:request", {
      requestId: randomUUID(),
      action,
      payload,
      respond: (raw: unknown) => {
        clearTimeout(timer);
        try {
          resolve(parseRespond(raw));
        } catch (e) {
          reject(e);
        }
      },
    });
  });
}

const handled = (r: Respond, what: string): unknown => {
  if (r.status !== "handled") throw new CpdError(`plannotator_${r.status}`, `plannotator answered ${what}: ${r.status}: ${errorText(r.error)}`);
  return r.result;
};

/** A `handled` answer to `plan-review` with `status: pending` gives plannotator's own review id; anything else is an error. */
export function parseOpened(r: Respond): string {
  const result = handled(r, "plan-review");
  if (!isObject(result) || result.status !== "pending" || typeof result.reviewId !== "string" || result.reviewId === "") {
    throw new CpdError("plannotator_invalid", `plannotator did not open the review: ${JSON.stringify(result)}`);
  }
  return result.reviewId;
}

export type ReviewStatus = { status: "pending" } | { status: "missing" } | { status: "completed"; approved: boolean; feedback?: string };

export function parseStatus(r: Respond): ReviewStatus {
  const result = handled(r, "review-status");
  if (isObject(result)) {
    if (result.status === "pending" || result.status === "missing") return { status: result.status };
    if (result.status === "completed" && typeof result.approved === "boolean") {
      return { status: "completed", approved: result.approved, ...(typeof result.feedback === "string" ? { feedback: result.feedback } : {}) };
    }
  }
  throw new CpdError("plannotator_invalid", `plannotator answered review-status with ${JSON.stringify(result)}`);
}

export type ReviewResult = { reviewId: string; approved: boolean; feedback?: string };

/** The `plannotator:review-result` payload, or undefined when it is not one (the channel is shared with plannotator's own flows). */
export function parseResult(data: unknown): ReviewResult | undefined {
  if (!isObject(data) || typeof data.reviewId !== "string" || typeof data.approved !== "boolean") return undefined;
  return { reviewId: data.reviewId, approved: data.approved, ...(typeof data.feedback === "string" ? { feedback: data.feedback } : {}) };
}
