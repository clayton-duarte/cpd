/** commands.md §15.4: the facts the adapter keeps and the state they give. */
export type Wanted = { state: "working" | "stopped" } | { state: "idle"; reply?: true } | { state: "blocked" | "waiting"; label: string };

export const LABEL_MAX = 80;

/** Blocked means it needs the human (commands.md §15.4): `human` = the User has to act (`dialog`, `φ: plan review`); `agent` = another agent handles it (`φ: Worker question`). */
export type Kind = "human" | "agent";

/** The first `Verdict` question's first option of an `ask_user_question` input (commands.md §15.4), tidied; none when the input is not of that shape. */
function verdictOption(input: unknown): string | undefined {
  const questions = (input as { questions?: unknown } | null)?.questions;
  if (!Array.isArray(questions)) return undefined;
  for (const q of questions) {
    if (typeof q?.header !== "string" || !/^verdict$/i.test(q.header)) continue;
    const first = Array.isArray(q.options) ? q.options[0]?.label : undefined;
    if (typeof first !== "string") continue;
    const option = first
      .replace(/[\s\p{Cc}]+/gu, " ")
      .trim()
      .replace(/(^| )\(Recommended\)$/i, "")
      .trim();
    if (option !== "") return option;
  }
  return undefined;
}

type Open = { key: string; label: string; kind: Kind };

/**
 * The reasons: its own (keyed, so opening twice or closing when not open changes nothing) and the dialog
 * reason (depth-counted), each of a kind.
 */
export class Facts {
  active = false;
  /** The run that is under way, or just ended, answered the User. Cleared at the next run start and once an `idle` that carries it is sent. */
  reply = false;
  private opened: Open[] = [];
  private prompts = 0;
  private own = 0;
  private verdict: string | undefined;

  open(key: string, label: string, kind: Kind): boolean {
    if (this.opened.some((o) => o.key === key)) return false;
    this.opened.push({ key, label, kind });
    return true;
  }

  close(key: string): boolean {
    const i = this.opened.findIndex((o) => o.key === key);
    if (i < 0) return false;
    this.opened.splice(i, 1);
    return true;
  }

  /** Runs one of the adapter's own prompts (the plan review's select and editor): the `ui_prompt_start` and `ui_prompt_end` that arrive meanwhile change no reason, the plan-review reason stands for it. */
  async ownPrompt<T>(show: () => Promise<T>): Promise<T> {
    this.own += 1;
    try {
      return await show();
    } finally {
      this.own -= 1;
    }
  }

  /** An accepted `ask_user_question` call: holds its Verdict option (if any) for the next dialog's label. */
  hold(input: unknown): void {
    this.verdict = verdictOption(input);
  }

  /** The `tool_result` of an `ask_user_question`: no dialog is left to name. */
  release(): void {
    this.verdict = undefined;
  }

  /** pi's `ui_prompt_start` (`active: true`) and `ui_prompt_end`: one reason, `dialog`, while any prompt is open. An end with nothing open changes nothing. */
  prompt(active: boolean): boolean {
    if (this.own > 0) return false;
    if (active) {
      this.prompts += 1;
      return this.prompts === 1 && this.open("ui_prompt", this.verdict === undefined ? "dialog" : `verdict: ${this.verdict}`, "human");
    }
    if (this.prompts === 0) return false;
    this.prompts -= 1;
    if (this.prompts > 0) return false;
    this.verdict = undefined;
    return this.close("ui_prompt");
  }

  /** `blocked` with the label of the latest human reason while any is open; else `waiting` with the label of the latest agent reason while any is open; else `working` while active; else `idle` (`reply` when the run answered the User). */
  wanted(): Wanted {
    const human = this.opened.findLast((o) => o.kind === "human");
    if (human) return { state: "blocked", label: human.label.slice(0, LABEL_MAX) || "blocked" };
    const agent = this.opened.findLast((o) => o.kind === "agent");
    if (agent) return { state: "waiting", label: agent.label.slice(0, LABEL_MAX) || "waiting" };
    if (this.active) return { state: "working" };
    return this.reply ? { state: "idle", reply: true } : { state: "idle" };
  }
}

/** `reply` is not part of the key: it rides on the one `idle` call that closes the run, and a later `idle` without it is not another call. */
export const wantedKey = (w: Wanted): string => (w.state === "blocked" || w.state === "waiting" ? `${w.state}:${w.label}` : w.state);

export const stateArgs = (w: Wanted, sessionId: string): string[] =>
  w.state === "blocked" || w.state === "waiting"
    ? ["session", "state", w.state, "--label", w.label, "--session", sessionId]
    : ["session", "state", w.state, ...(w.state === "idle" && w.reply ? ["--reply"] : []), "--session", sessionId];

/**
 * Serialised reporting: at most one call in flight; while it runs only the latest wanted state is kept and sent next,
 * so states never arrive out of order. A refused call goes to `onError`; the next change is tried normally.
 */
export class Reporter {
  private lastKey: string | undefined;
  private pending: Wanted | undefined;
  private inflight: Promise<void> | undefined;
  private closed = false;
  private readonly send: (w: Wanted) => Promise<void>;
  private readonly onError: (e: unknown) => void;

  constructor(send: (w: Wanted) => Promise<void>, onError: (e: unknown) => void) {
    this.send = send;
    this.onError = onError;
  }

  want(w: Wanted): void {
    if (this.closed) return;
    this.pending = w;
    this.pump();
  }

  /** Resolves when nothing is in flight and nothing is pending. */
  async settled(): Promise<void> {
    while (this.inflight) await this.inflight;
  }

  /** Wait for the call in flight, drop what is pending, then send `final` (the call is made even if the state is unchanged). */
  async close(final: Wanted): Promise<void> {
    this.closed = true;
    this.pending = undefined;
    await this.settled();
    try {
      await this.send(final);
    } catch (e) {
      this.onError(e);
    }
  }

  private pump(): void {
    if (this.inflight || !this.pending) return;
    const w = this.pending;
    this.pending = undefined;
    if (wantedKey(w) === this.lastKey) return;
    this.lastKey = wantedKey(w);
    this.inflight = this.send(w)
      .catch((e) => {
        this.lastKey = undefined; // refused: the same state is tried again when wanted again
        this.onError(e);
      })
      .finally(() => {
        this.inflight = undefined;
        this.pump();
      });
  }
}
