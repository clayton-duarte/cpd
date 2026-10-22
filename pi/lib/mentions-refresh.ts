import type { Host } from "./host.ts";
import { type Painter, mentionIds, paintLine, PAINTER_KEY } from "./mentions.ts";
import type { Run } from "./run.ts";

const holder = globalThis as typeof globalThis & { [PAINTER_KEY]?: Painter };

/** §15.14: reloads the ids (a failed command keeps the previous set and is delivered) and publishes the painter of a Lead's or Manager's pane. */
export async function refreshMentions(host: Host, r: Run): Promise<void> {
  if (r.info.role !== "lead" && r.info.role !== "manager") return;
  try {
    const [s, p, c] = await Promise.all([
      host.call(["session", "list", "--archived", "--archived", "--role", "lead", "--role", "manager"]),
      host.call(["plan", "list"]),
      host.call(["context", "list"]),
    ]);
    if (host.run !== r) return;
    const ids = mentionIds(s.sessions, p.plans, c.contexts);
    const role = r.info.role;
    holder[PAINTER_KEY] = (line) => {
      const theme = (host.ctx?.ui as { theme?: { getFgAnsi?: (k: string) => string } } | undefined)?.theme;
      const codeOpens = theme?.getFgAnsi ? ["mdCode", "mdCodeBlock"].map((k) => theme.getFgAnsi!(k)) : [];
      return paintLine(line, ids, codeOpens, role);
    };
  } catch (e) {
    host.deliver("mentions", e);
  }
}

export function clearMentions(): void {
  delete holder[PAINTER_KEY];
}
