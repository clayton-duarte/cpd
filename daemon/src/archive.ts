import { defineDoc, type RewindableConversationDocToken } from "@earendil-works/pi-durable";
import type { JsonValue } from "@earendil-works/chord";

// --- L4: archived is a CPD-owned flag, never a pi-durable schema change -----------------------
//
// pi-durable@1.1.0 has no deleteConversation/removeConversation/archiveConversation -- the
// transcript store is append-only by design (verified against dist: no such export exists
// anywhere). Archiving must live entirely in our layer and must never touch pi's own
// `entries`/`conversations` tables. Following the same pattern H4 already established for plan
// jobs (`PlanDoc` in plans.ts), this is a second rewindable, fork-copy-on-write document scoped
// per conversation -- not a new sqlite table -- since pi's document API supports exactly this
// app-defined-state-next-to-the-transcript use case and reusing it means one less schema to
// migrate/own.

export type ArchiveState = {
  archived: boolean;
  [key: string]: JsonValue;
};

export const ArchiveDoc: RewindableConversationDocToken<ArchiveState> = defineDoc({
  kind: "cpd.archive",
  version: 1,
  scope: "conversation",
  history: "rewindable",
  fork: "asOf",
  initial: (): ArchiveState => ({ archived: false }),
});
