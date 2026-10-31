// Contract §6.18 "Retier": apply a `session-retiered` change at idle, in one commit, and report the
// outcome to the engine. run.ts catches the Event and resolves the agent change; this module owns the
// idle guard and the reporting. This is a stub: the test Job types the exports, the impl fills them.

export type RetierPort = {
  /** The host is idle with nothing queued (the lib's `idle` adapted to the host's own state). */
  idle(): boolean;
  /** pi-durable's `configure({ model, thinkingLevel }, ctx)`: the host's one commit. */
  configure(agentChange: unknown): Promise<void>;
  /** `cpd session switched` / `cpd session switch-failed` (commands.md §10.18a/b). */
  report(kind: "switched" | "switch-failed", detail?: string): Promise<void>;
};

/** At idle: configure then report `switched`; while busy: report `switch-failed`, configure nothing. */
export function createRetier(port: RetierPort): { apply(event: unknown): Promise<void> } {
  return {
    async apply(): Promise<void> {
      throw new Error("not implemented");
    },
  };
}
