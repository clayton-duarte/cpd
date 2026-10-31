/** Contract §6.13 "Delivery (P1.4)", steps 3 to 6: what the host stores before an input that starts a turn, the turn-end reminder and the host errors. Effects go through `TurnPort`. */

/** An entry as the conversation stores it (`Conversation.entries`): its `kind`, `data` and the model messages it carries. */
export type StoredEntry = { kind: string; data?: Record<string, unknown>; model?: readonly unknown[] };

/** One `conversation.submit({ type: "write", ... })`. `model: []` is a silent write that changes nothing the model sees. */
export type Write = { type: "write"; entry: { kind: string; data: Record<string, unknown>; model: readonly unknown[] }; requestId?: string };

/** The Session the host runs, as `cpd session show` gives it. `byUser` is true when the User created it (it has a first message). */
export type Seat = { id: string; role: "lead" | "manager"; context: string; plan: string | null; registeredEvent: number; byUser: boolean };

/** Every effect of this module. */
export type TurnPort = {
  seat: Seat;
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  submit(write: Write): Promise<unknown>;
  entries(): Promise<readonly StoredEntry[]>;
  log(line: string): void;
};

/** What lives for one run of the host: the "once" memories of §6.13. */
export type TurnState = { firstFailed: boolean; firstEvent: number; contextFailed: boolean; lastError: string | undefined; reminded: boolean };

export const newTurnState = (): TurnState => ({ firstFailed: false, firstEvent: 0, contextFailed: false, lastError: undefined, reminded: false });

/** A host error: `what` and `code` are the memory's key. */
export type HostError = { what: string; code: string; message: string; details?: Record<string, unknown> };

/** Before an input that starts a turn: stores the first message while it is pending, then the context update, each as one write. A failed read is reported once and never throws. */
export async function prepareTurn(_port: TurnPort, _state: TurnState): Promise<void> {
  throw new Error("not implemented");
}

/** At a run's end (`onYield`): the text to continue the run with, or undefined. Plan Leads only, once per run. `messages` is the run's messages. */
export async function reminderFor(_messages: unknown, _port: TurnPort, _state: TurnState): Promise<string | undefined> {
  throw new Error("not implemented");
}

/** A Lead's error is one `cpd session report`; a Manager's one silent `cpd-error` write; once per (`what`, `code`) in a row. */
export async function reportHostError(_error: HostError, _port: TurnPort, _state: TurnState): Promise<void> {
  throw new Error("not implemented");
}
