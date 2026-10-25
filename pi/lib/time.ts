/**
 * commands.md §15.3, §15.7: the one formatter for a stored ISO 8601 UTC `at`. The adapter renders the absolute
 * machine-local time with Node's `Date` local methods, so the process timezone decides and no offset is hardcoded.
 */

const pad = (n: number): string => String(n).padStart(2, "0");

/** The local date and time parts of `at`: `YYYY-MM-DD` and `HH:MM`. */
function localParts(at: string): { date: string; time: string } {
  const d = new Date(at);
  return {
    date: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`,
    time: `${pad(d.getHours())}:${pad(d.getMinutes())}`,
  };
}

/** `HH:MM`, 24-hour, zero-padded, no seconds, in the process's local time. */
export function localTime(at: string): string {
  return localParts(at).time;
}

/** `YYYY-MM-DD HH:MM`, the local date and time, in the process's local time. */
export function localDateTime(at: string): string {
  const { date, time } = localParts(at);
  return `${date} ${time}`;
}
