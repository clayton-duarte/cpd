import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { readSessionHeader } from "../herdr/session.ts";
import { refused, usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { assertSessionId, leadDir, leadQueueDir } from "../lib/paths.ts";
import { LeadMessageSchema } from "../queue/messages.ts";

const NAME_MAX = 80;

function assertName(name: string): void {
  const chars = [...name].length;
  if (chars < 1 || chars > NAME_MAX) throw usage("invalid_name", `session name must be 1-${NAME_MAX} characters, got ${chars}`);
  if (/\p{Cc}/u.test(name)) throw usage("invalid_name", "session name must not contain control characters");
}

/** A `--session` value is a path when it has a directory part or a `.jsonl` suffix, else a session id. */
function sessionIdOf(target: string): string {
  if (target.includes("/") || target.endsWith(".jsonl")) return readSessionHeader(target).id;
  assertSessionId(target);
  if (!existsSync(leadDir(target))) throw refused("session_unknown", `no cpd queue for session id ${target}`, { session: target });
  return target;
}

/** Contract §11: queue a `rename` message for a pi session; the pi adapter applies it. */
export async function run(argv: string[]): Promise<Result> {
  let parsed: ReturnType<typeof parseArgs<{ options: { session: { type: "string" } }; allowPositionals: true }>>;
  try {
    parsed = parseArgs({ args: argv, options: { session: { type: "string" } }, allowPositionals: true, strict: true });
  } catch (e) {
    throw usage("usage", `cpd session rename: ${(e as Error).message}`);
  }
  const [name, ...extra] = parsed.positionals;
  if (name === undefined || extra.length > 0) throw usage("usage", "usage: cpd session rename <name> [--session <id|path>]");
  assertName(name);

  const target = parsed.values.session ?? (process.env.PI_SESSION_FILE || undefined);
  if (!target) throw refused("session_unknown", "no session: run inside a pi session (PI_SESSION_FILE) or pass --session <id|path>");
  const sessionId = sessionIdOf(target);

  const message = LeadMessageSchema.parse({
    v: 1,
    id: randomUUID(),
    kind: "rename",
    text: `Session renamed to "${name}".`,
    data: { name },
    createdAt: new Date().toISOString(),
  });
  const queueDir = leadQueueDir(sessionId);
  mkdirSync(queueDir, { recursive: true });
  const path = join(queueDir, `${Date.now()}-${message.id}.json`);
  const tmp = `${path}.tmp`;
  writeFileSync(tmp, JSON.stringify(message, null, 2) + "\n", { flag: "wx" });
  renameSync(tmp, path);
  return { ok: true, sessionId, name };
}
