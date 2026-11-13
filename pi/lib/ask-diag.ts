import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

/** Appends one line to the diag log, creating its directory. May throw; callers swallow. */
export function appendDiagLine(file: string, line: string): void {
	mkdirSync(dirname(file), { recursive: true });
	appendFileSync(file, `${line}\n`);
}
