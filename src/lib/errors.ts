export class CpdError extends Error {
  readonly code: string;
  readonly details: Record<string, unknown>;
  readonly exitCode: number;
  constructor(code: string, message: string, exitCode: number, details: Record<string, unknown> = {}) {
    super(message);
    this.name = "CpdError";
    this.code = code;
    this.details = details;
    this.exitCode = exitCode;
  }
}

type Details = Record<string, unknown>;

/** exit 1: external tool failure or unexpected. */
export const failure = (code: string, message: string, details: Details = {}) =>
  new CpdError(code, message, 1, details);
/** exit 2: usage error. */
export const usage = (code: string, message: string, details: Details = {}) =>
  new CpdError(code, message, 2, details);
/** exit 3: refused by an invariant. */
export const refused = (code: string, message: string, details: Details = {}) =>
  new CpdError(code, message, 3, details);
/** exit 4: not found. */
export const notFound = (code: string, message: string, details: Details = {}) =>
  new CpdError(code, message, 4, details);
/** exit 5: timeout. */
export const timeout = (code: string, message: string, details: Details = {}) =>
  new CpdError(code, message, 5, details);
