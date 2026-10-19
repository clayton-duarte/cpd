/** commands.md §15.4: the facts the adapter keeps and the state they give. */
export type Wanted = { state: "working" | "stopped" } | { state: "idle"; reply?: true } | { state: "blocked"; label: string };

export const LABEL_MAX = 80;

type Open = { key: string; label: string };

/**
 * The blocking reasons: its own (keyed, so opening twice or closing when not open changes nothing) and the dialog
 * reason (depth-counted).
 */
export class Facts {
  active = false;
  /** The run that is under way, or just ended, answered the User. Cleared at the next run start and once an `idle` that carries it is sent. */
  reply = false;
  private opened: Open[] = [];
  private prompts = 0;

  open(key: string, label: string): boolean {
    if (this.opened.some((o) => o.key === key)) return false;
    this.opened.push({ key, label });
    return true;
  }

  close(key: string): boolean {
    const i = this.opened.findIndex((o) => o.key === key);
    if (i < 0) return false;
    this.opened.splice(i, 1);
    return true;
  }

  /** pi's `ui_prompt_start` (`active: true`) and `ui_prompt_end`: one reason, `dialog`, while any prompt is open. An end with nothing open changes nothing. */
  prompt(active: boolean): boolean {
    if (active) {
      this.prompts += 1;
      return this.prompts === 1 && this.open("ui_prompt", "dialog");
    }
    if (this.prompts === 0) return false;
    this.prompts -= 1;
    return this.prompts === 0 && this.close("ui_prompt");
  }

  /** `blocked` with the label of the most recently opened reason while any is open; else `working` while active; else `idle` (`reply` when the run answered the User). */
  wanted(): Wanted {
    const last = this.opened.at(-1);
    if (last) return { state: "blocked", label: last.label.slice(0, LABEL_MAX) || "blocked" };
    if (this.active) return { state: "working" };
    return this.reply ? { state: "idle", reply: true } : { state: "idle" };
  }
}

/** `reply` is not part of the key: it rides on the one `idle` call that closes the run, and a later `idle` without it is not another call. */
export const wantedKey = (w: Wanted): string => (w.state === "blocked" ? `blocked:${w.label}` : w.state);

export const stateArgs = (w: Wanted, sessionId: string): string[] =>
  w.state === "blocked"
    ? ["session", "state", "blocked", "--label", w.label, "--session", sessionId]
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
