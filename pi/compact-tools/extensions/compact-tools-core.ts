/** Pure timing and text helpers for compact tool rendering. */

export type RowStatus = "pending" | "running" | "asking" | "success" | "error";
// "asking": a tool waits for the User to decide (the ask_user_question window); only its pill is painted differently.

export function indicatorGlyph(_status: RowStatus, _frame = 0): string {
	return "⦁";
}

export function normalizeLineEndings(value: string): string {
	return value.replace(/\r\n?|\n/g, "\n");
}

export function classifyCallStatus(isError: boolean, executionStarted: boolean, completed: boolean): RowStatus {
	if (isError) return "error";
	if (completed) return "success";
	return executionStarted ? "running" : "pending";
}

/**
 * Sub-minute durations keep millisecond precision; longer runs switch to the
 * minute and hour grouping Pi's own bash and powershell renderers use.
 */
export function formatDurationMs(elapsedMs: number): string {
	const seconds = elapsedMs / 1000;
	if (seconds < 60) return `${seconds.toFixed(3)}s`;
	const totalSeconds = Math.floor(seconds);
	const minutes = Math.floor(totalSeconds / 60);
	const remainderSeconds = totalSeconds % 60;
	if (minutes < 60) return `${minutes}m ${remainderSeconds}s`;
	return `${Math.floor(minutes / 60)}h ${minutes % 60}m ${remainderSeconds}s`;
}
