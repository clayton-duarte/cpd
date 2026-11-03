import type { JSX } from "react";
import type { QueuedSubmission, TranscriptItem, WhenBusy, WireError } from "../../protocol/domain.ts";

// Structural copy of PendingSubmission (web/src/state/store.ts, contract 6.2), which does not exist yet.
export interface PendingSubmission {
  readonly clientSubmissionId: string;
  readonly text: string;
  readonly imageCount: number;
  readonly whenBusy: WhenBusy;
  readonly state: "sending" | "failed";
  readonly error?: WireError;
}

export function Transcript(_props: {
  items: readonly TranscriptItem[];
  hasEarlier: boolean;
  queued: readonly QueuedSubmission[];
  pending: readonly PendingSubmission[];
  onLoadEarlier(): void;
}): JSX.Element {
  throw new Error("not implemented");
}
