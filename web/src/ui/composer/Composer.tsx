import type { Ref } from "react";
import type { Attachment, Capabilities, QueuedSubmission, WhenBusy } from "../../protocol/domain.ts";
import type { FileLike, FileReaderBoundary } from "./attachments.ts";

export interface ComposerHandle { addFiles(files: readonly FileLike[]): Promise<void> }

export function Composer(_props: {
  capabilities: Capabilities | null; busy: boolean; queued: readonly QueuedSubmission[];
  reader: FileReaderBoundary;
  onSubmit(input: { text: string; attachments: readonly Attachment[]; whenBusy: WhenBusy }): void;
  onAbort(): void; onCancelQueued(clientSubmissionId: string): void;
  ref?: Ref<ComposerHandle>;
}): React.JSX.Element {
  throw new Error("not implemented");
}
