/**
 * The refusal miner (contract §5.7; pi/tools/refusal-miner/README.md is the worked contract).
 *
 * Pure: the contents of pi session files plus a `since` cutoff in, the ranking tables out. It reads
 * no store, runs no command and writes no file.
 *
 * Two families of refusal:
 *  - an adapter guard refusal: a `toolResult` whose first content `text` starts with `φ: `
 *    (U+03C6 + a space), `isError: true`, `details: {}` — the call never ran; the guard is the
 *    text's exact prefix (README's guard table).
 *  - a CLI refusal: a `bash`/`codemode` `toolResult` whose output carries the envelope
 *    `{"ok":false,"error":{"code":"<code>",…}}` at the start of a line, counted once per envelope.
 * A refusal only quoted inside another output (an envelope not at the start of a line, or the whole
 * output of a pure evidence reader) is not counted.
 *
 * A record counts when its top-level `timestamp` (ISO-8601, UTC `Z`) is at or after `since`.
 *
 * Role is read from the session itself: the loaded cpd skill (`cpd-lead` → Lead, `cpd-manager` →
 * Manager); otherwise a session whose `cwd` is an explore directory (`…/run/<plan>/explore`) is a
 * scout, and every other session is a Worker. The model is the session's last `model_change`
 * (`provider/modelId`) at or before the refusal.
 */

export type Role = "Manager" | "Lead" | "Worker" | "scout";

/** One session file: the file's name (its directory key and file) and its raw contents. */
export interface SessionFile {
  name: string;
  text: string;
}

export interface GuardRow {
  /** `φ: <guard>` for an adapter guard, `cli: <code>` for a CLI refusal. */
  guard: string;
  /** The exact matched string the row quotes: the refusal prefix, or the envelope shape. */
  sample: string;
  count: number;
}

export interface RoleRow {
  guard: string;
  role: Role;
  count: number;
}

export interface ModelRow {
  guard: string;
  model: string;
  count: number;
}

export interface Tables {
  /** Highest count first, then guard name. */
  byGuard: GuardRow[];
  /** Guard name, then role in the order Manager, Lead, Worker, scout. */
  byRole: RoleRow[];
  /** Guard name, then model. */
  byModel: ModelRow[];
  total: number;
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);

const ROLES: readonly Role[] = ["Manager", "Lead", "Worker", "scout"];
const ROLE_RANK: Record<Role, number> = { Manager: 0, Lead: 1, Worker: 2, scout: 3 };
const CLI_PREFIX = '{"ok":false';

/**
 * The adapter guards, matched by the exact prefix of the refusal text (the README guard table;
 * `pi/lib/adapter.ts` wires each one). The list is ordered longest prefix first so a specific
 * guard (`explore network write`) wins over its shorter sibling (`explore network`).
 */
const ADAPTER_GUARDS: readonly { guard: string; prefix: string }[] = [
  { guard: "loop_guard_read_streak (>12)", prefix: "φ: loop guard: this tool call was NOT executed: more than 12 consecutive read-only tool calls in one turn." },
  { guard: "loop_guard_read_streak (>30)", prefix: "φ: loop guard: this tool call was NOT executed: more than 30 read-only tool calls in one turn." },
  { guard: "noblock_lead", prefix: "φ: a Lead never blocks: " },
  { guard: "noblock_manager", prefix: "φ: a Manager never blocks: " },
  { guard: "explore_bash_timeout", prefix: "φ: explore timeout: " },
  { guard: "explore_write", prefix: "φ: explore write: " },
  { guard: "explore_network_write", prefix: "φ: explore network write: " },
  { guard: "explore_network", prefix: "φ: explore network: " },
  { guard: "explore_scope", prefix: "φ: explore scope: " },
  { guard: "explore_secret", prefix: "φ: explore secret: " },
  { guard: "explore_artefact_read", prefix: "φ: explore artefact: " },
  { guard: "primary_checkout", prefix: "φ: no agent moves HEAD or changes the files of a repo's primary checkout" },
  { guard: "external_writes", prefix: "φ: external writes are the User's: " },
  { guard: "host_approve", prefix: "φ: approvals are the User's: ask with the tool cpd_host_approve" },
  { guard: "secret_approve", prefix: "φ: approvals are the User's: ask with the tool cpd_secret_approve" },
  { guard: "secret_op", prefix: "φ: agents do not call 1Password's op" },
  { guard: "electron", prefix: "φ: agents do not run the electron binary" },
  { guard: "full_test_run", prefix: "φ: a Worker runs only the tests its change touches" },
  { guard: "ask_user_question_guard", prefix: "φ: ask_user_question is for a few short choices;" },
  { guard: "ask_user_question_guard", prefix: "φ: state what you ask about in the question itself" },
]
  .slice()
  .sort((a, b) => b.prefix.length - a.prefix.length);

/** The guard and matched prefix of an adapter refusal text; an unlisted guard falls back to its own leading clause. */
function adapterGuard(text: string): { guard: string; prefix: string } {
  for (const s of ADAPTER_GUARDS) if (text.startsWith(s.prefix)) return s;
  const rest = text.slice(3);
  const i = rest.indexOf(":");
  return { guard: i === -1 ? rest.slice(0, 80) : rest.slice(0, i), prefix: text };
}

/**
 * One command's output is a pure evidence reader when it only reads back recorded refusals (the
 * scout-ranking contamination control): `cpd events list`, the context digests, the title logs and
 * the session files themselves. Such an output carries refusals it is quoting, never making them.
 */
const EVIDENCE_READERS: readonly RegExp[] = [
  /\bcpd events (?:list|follow)\b/,
  /context-digests/,
  /\/run\/titles\//,
  /\.pi\/agent\/sessions/,
];

const isEvidenceReader = (command: string | undefined): boolean =>
  command !== undefined && EVIDENCE_READERS.some((re) => re.test(command));

/** The CLI refusals on the output of one `bash`/`codemode` toolResult: one per envelope at the start of a line. */
function cliRefusals(text: string): { guard: string; sample: string }[] {
  const out: { guard: string; sample: string }[] = [];
  for (const line of text.split("\n")) {
    if (!line.startsWith(CLI_PREFIX)) continue;
    let v: unknown;
    try {
      v = JSON.parse(line);
    } catch {
      continue;
    }
    if (!isObj(v) || v.ok !== false) continue;
    const err = v.error;
    const code = isObj(err) && typeof err.code === "string" ? err.code : undefined;
    out.push({ guard: code === undefined ? "cli: (no code field)" : `cli: ${code}`, sample: line });
  }
  return out;
}

/** The text of a `toolResult` message's first text block, or undefined. */
function firstText(message: Obj): string | undefined {
  const content = message.content;
  if (!Array.isArray(content)) return undefined;
  const first = content[0];
  return isObj(first) && first.type === "text" && typeof first.text === "string" ? first.text : undefined;
}

type Refusal = { guard: string; sample: string; role: Role; model: string };

/** The role a session holds: its loaded cpd skill, else an explore cwd is a scout and anything else a Worker. */
function roleOf(skill: string | undefined, cwd: string | undefined): Role {
  if (skill === "cpd-lead") return "Lead";
  if (skill === "cpd-manager") return "Manager";
  if (cwd !== undefined && /(?:^|\/)run\/[^/]+\/explore(?:\/|$)/.test(cwd)) return "scout";
  return "Worker";
}

const cmp = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);

/** Mine the refusals in `files` whose top-level timestamp is at or after `options.since`. */
export function mineRefusals(files: SessionFile[], options: { since: string }): Tables {
  const since = Date.parse(options.since);
  const refusals: Refusal[] = [];

  for (const file of files) {
    let skill: string | undefined;
    let cwd: string | undefined;
    let model = "unknown";
    const commands = new Map<string, string>();

    for (const line of file.text.split("\n")) {
      if (line.trim() === "") continue;
      let record: unknown;
      try {
        record = JSON.parse(line);
      } catch {
        continue;
      }
      if (!isObj(record)) continue;

      if (record.type === "session") {
        skill = undefined;
        cwd = typeof record.cwd === "string" ? record.cwd : undefined;
        model = "unknown";
        commands.clear();
        continue;
      }

      if (record.type === "custom" && record.customType === "cpd-skills" && isObj(record.data) && typeof record.data.skill === "string") {
        skill = record.data.skill;
        continue;
      }

      if (record.type === "model_change" && typeof record.provider === "string" && typeof record.modelId === "string") {
        model = `${record.provider}/${record.modelId}`;
        continue;
      }

      if (record.type !== "message" || !isObj(record.message)) continue;
      const message = record.message;

      if (message.role === "assistant" && Array.isArray(message.content)) {
        for (const block of message.content) {
          if (isObj(block) && block.type === "toolCall" && typeof block.id === "string" && isObj(block.arguments) && typeof block.arguments.command === "string") {
            commands.set(block.id, block.arguments.command);
          }
        }
        continue;
      }

      if (message.role !== "toolResult") continue;

      const ts = typeof record.timestamp === "string" ? Date.parse(record.timestamp) : NaN;
      if (Number.isNaN(ts) || ts < since) continue;

      const role = roleOf(skill, cwd);
      const text = firstText(message);
      if (text === undefined) continue;

      if (message.isError === true && text.startsWith("φ: ")) {
        const found = adapterGuard(text);
        refusals.push({ guard: `φ: ${found.guard}`, sample: found.prefix, role, model });
        continue;
      }

      const toolName = typeof message.toolName === "string" ? message.toolName : "";
      if (toolName !== "bash" && toolName !== "codemode") continue;
      const callId = typeof message.toolCallId === "string" ? message.toolCallId : "";
      if (isEvidenceReader(commands.get(callId))) continue;
      for (const r of cliRefusals(text)) refusals.push({ ...r, role, model });
    }
  }

  const guardCounts = new Map<string, { sample: string; count: number }>();
  const roleCounts = new Map<string, number>();
  const modelCounts = new Map<string, number>();
  for (const r of refusals) {
    const g = guardCounts.get(r.guard);
    if (g === undefined) guardCounts.set(r.guard, { sample: r.sample, count: 1 });
    else g.count += 1;
    roleCounts.set(`${r.guard}\u0000${r.role}`, (roleCounts.get(`${r.guard}\u0000${r.role}`) ?? 0) + 1);
    modelCounts.set(`${r.guard}\u0000${r.model}`, (modelCounts.get(`${r.guard}\u0000${r.model}`) ?? 0) + 1);
  }

  const byGuard: GuardRow[] = [...guardCounts]
    .map(([guard, v]) => ({ guard, sample: v.sample, count: v.count }))
    .sort((a, b) => b.count - a.count || cmp(a.guard, b.guard));

  const byRole: RoleRow[] = [...roleCounts]
    .map(([key, count]) => {
      const at = key.indexOf("\u0000");
      return { guard: key.slice(0, at), role: key.slice(at + 1) as Role, count };
    })
    .sort((a, b) => cmp(a.guard, b.guard) || ROLE_RANK[a.role] - ROLE_RANK[b.role]);

  const byModel: ModelRow[] = [...modelCounts]
    .map(([key, count]) => {
      const at = key.indexOf("\u0000");
      return { guard: key.slice(0, at), model: key.slice(at + 1), count };
    })
    .sort((a, b) => cmp(a.guard, b.guard) || cmp(a.model, b.model));

  return { byGuard, byRole, byModel, total: refusals.length };
}
