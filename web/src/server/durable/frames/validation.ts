import { WebError } from "../../../protocol/domain.ts";

export type Obj = Record<string, unknown>;
export const protocolError = (message: string): never => {
  throw new WebError("protocol_error", message);
};
export const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
export const obj = (v: unknown, what: string): Obj => (isObj(v) ? v : protocolError(`${what} is not an object`));
export const str = (v: unknown, what: string): string => (typeof v === "string" ? v : protocolError(`${what} is not a string`));
export const arr = (v: unknown, what: string): unknown[] => (Array.isArray(v) ? v : protocolError(`${what} is not an array`));

export const num = (v: unknown, what: string): number => (typeof v === "number" && Number.isFinite(v) ? v : protocolError(`${what} is not a number`));
