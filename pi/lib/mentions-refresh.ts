import { toCpdError } from "./cpd.ts";
import type { Host } from "./host.ts";
import { type Painter, mentionIds, paintLine, PAINTER_KEY } from "./mentions.ts";
import type { Run } from "./run.ts";

const holder = globalThis as typeof globalThis & { [PAINTER_KEY]?: Painter };

/** The last delivered failure (code and message): a failure that repeats the same way is shown once; a success clears it. */
let lastFailure: string | undefined;

/** §15.14: reloads the ids (a failed command keeps the previous set and is delivered) and publishes the painter of a Lead's or Manager's pane. */
export async function refreshMentions(host: Host, r: Run): Promise<void> {
  if (r.info.role !== "lead" && r.info.role !== "manager") return;
  try {
    const [s, p, c] = await Promise.all([
      host.call(["session", "list", "--archived", "--role", "lead", "--role", "manager"]),
      host.call(["plan", "list"]),
      host.call(["context", "list"]),
    ]);
    if (host.run !== r) return;
    const ids = mentionIds(s.sessions, p.plans, c.contexts);
    lastFailure = undefined;
    const role = r.info.role;
    holder[PAINTER_KEY] = (line) => {
      const theme = (host.ctx?.ui as { theme?: { getFgAnsi?: (k: string) => string } } | undefined)?.theme;
      const codeOpens = theme?.getFgAnsi ? ["mdCode", "mdCodeBlock"].map((k) => theme.getFgAnsi!(k)) : [];
      return paintLine(line, ids, codeOpens, role);
    };
  } catch (e) {
    const err = toCpdError(e);
    const key = `${err.code}\n${err.message}`;
    if (key === lastFailure) return;
    lastFailure = key;
    host.deliver("mentions", e);
  }
}

export function clearMentions(): void {
  delete holder[PAINTER_KEY];
}
