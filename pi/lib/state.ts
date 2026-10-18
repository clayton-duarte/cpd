/** commands.md §15.4: the facts the adapter keeps and the state they give. */
export type Wanted = { state: "working" | "idle" | "stopped" } | { state: "blocked"; label: string };

export const LABEL_MAX = 80;

type Open = { key: string; label: string };

/**
 * The blocking reasons: its own (keyed, so opening twice or closing when not open changes nothing) and other
 * extensions' `herdr:blocked` signals (counted: one `active: false` closes one `active: true`).
 */
export class Facts {
  active = false;
  private opened: Open[] = [];
  private signals = 0;
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

  /** A `herdr:blocked` signal. Each active one is an entry of its own, so the label of the latest wins. */
  signal(active: boolean, label: string | undefined): void {
    if (active) {
      this.signals += 1;
      this.opened.push({ key: `signal:${this.signals}`, label: label ?? "blocked" });
      return;
    }
    // An `active: false` with nothing open closes nothing.
    for (let i = this.opened.length - 1; i >= 0; i--) {
      if (this.opened[i]!.key.startsWith("signal:")) {
        this.opened.splice(i, 1);
        return;
      }
    }
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

  /** `blocked` with the label of the most recently opened reason while any is open; else `working` while active; else `idle`. */
  wanted(): Wanted {
    const last = this.opened.at(-1);
    if (last) return { state: "blocked", label: last.label.slice(0, LABEL_MAX) || "blocked" };
    return { state: this.active ? "working" : "idle" };
  }
}

export const wantedKey = (w: Wanted): string => (w.state === "blocked" ? `blocked:${w.label}` : w.state);

export const stateArgs = (w: Wanted, sessionId: string): string[] =>
  w.state === "blocked" ? ["session", "state", "blocked", "--label", w.label, "--session", sessionId] : ["session", "state", w.state, "--session", sessionId];

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
      .catch(this.onError)
      .finally(() => {
        this.inflight = undefined;
        this.pump();
      });
  }
}
