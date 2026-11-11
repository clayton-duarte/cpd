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

const FIELDS: Intl.DateTimeFormatOptions = { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hourCycle: "h23" };

function parts(at: string | Date, timeZone: string | undefined, timeZoneName: "short" | "longOffset"): Record<string, string> {
  const out: Record<string, string> = {};
  for (const p of new Intl.DateTimeFormat("en-CA", { ...FIELDS, timeZone, timeZoneName }).formatToParts(new Date(at))) out[p.type] = p.value;
  return out;
}

/** ZZZ: the zone abbreviation when it is letters only, otherwise the numeric offset `+05:30`. */
function zoneLabel(at: string | Date, timeZone: string | undefined): string {
  const short = parts(at, timeZone, "short").timeZoneName ?? "";
  if (/^[A-Za-z]+$/.test(short)) return short;
  const off = (parts(at, timeZone, "longOffset").timeZoneName ?? "").replace(/^GMT/, "");
  return off === "" ? "+00:00" : off;
}

/** The PROSE FORMAT `YYYY-MM-DD HH:MM ZZZ`, in the system zone (or `timeZone`). */
export function proseTime(at: string | Date, timeZone?: string): string {
  const p = parts(at, timeZone, "short");
  return `${p.year}-${p.month}-${p.day} ${p.hour}:${p.minute} ${zoneLabel(at, timeZone)}`;
}

/** `HH:MM ZZZ`. */
export function zoneTime(at: string | Date, timeZone?: string): string {
  const p = parts(at, timeZone, "short");
  return `${p.hour}:${p.minute} ${zoneLabel(at, timeZone)}`;
}

/** The system prompt line naming the zone, built from `now`. */
export function zoneLine(now: Date, timeZone?: string): string {
  const iana = timeZone ?? new Intl.DateTimeFormat().resolvedOptions().timeZone; // Node canonicalises a given name (Asia/Kolkata -> Asia/Calcutta), so a given one is kept
  return `Time zone: ${iana} (${zoneLabel(now, timeZone)}). State every time to a person in this zone with the zone named, as ${proseTime(STATIC_REFERENCE_TIME, timeZone)}, never in UTC.`;
}

export const TIME_ZONE_SECTION = "cpd-time-zone";

const STATIC_REFERENCE_TIME = new Date("2026-01-15T12:00:00.000Z");

