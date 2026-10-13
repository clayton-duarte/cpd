import { CpdError } from "./errors.ts";

export type Result =
  | ({ ok: true } & Record<string, unknown>)
  | {
      ok: false;
      error: { code: string; message: string; details: Record<string, unknown> };
      exitCode: number;
    };

export function errorResult(e: CpdError): Result {
  return { ok: false, error: { code: e.code, message: e.message, details: e.details }, exitCode: e.exitCode };
}

/** Print exactly one JSON line to stdout and set the process exit code (contract §2). */
export function emit(result: Result): void {
  if (result.ok) {
    process.stdout.write(JSON.stringify(result) + "\n");
    process.exitCode = 0;
  } else {
    const { exitCode, ...body } = result;
    process.stdout.write(JSON.stringify(body) + "\n");
    process.exitCode = exitCode;
  }
}
