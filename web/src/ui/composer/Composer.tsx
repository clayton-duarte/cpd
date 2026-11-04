import { useImperativeHandle, useRef, useState } from "react";
import type { ClipboardEvent, DragEvent, KeyboardEvent, ReactNode, Ref } from "react";
import type { Attachment, Capabilities, QueuedSubmission, Support, WhenBusy } from "../../protocol/domain.ts";
import { addFiles, filesFromClipboard, filesFromDrop } from "./attachments.ts";
import type { AttachmentRejection, FileLike, FileReaderBoundary } from "./attachments.ts";

export interface ComposerHandle { addFiles(files: readonly FileLike[]): Promise<void> }

const hasFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes("Files");
const reasonOf = (s: Support | undefined): string | null => (s === undefined || s.supported ? null : s.reason);

export function Composer(props: {
  capabilities: Capabilities | null; busy: boolean; queued: readonly QueuedSubmission[];
  reader: FileReaderBoundary;
  onSubmit(input: { text: string; attachments: readonly Attachment[]; whenBusy: WhenBusy }): void;
  onAbort(): void; onCancelQueued(clientSubmissionId: string): void;
  ref?: Ref<ComposerHandle>;
  settings?: ReactNode; label?: string; onEditQueued?(clientSubmissionId: string): void;
}): React.JSX.Element {
  const { capabilities: caps, busy, queued, reader } = props;
  const [text, setText] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [rejected, setRejected] = useState<AttachmentRejection[]>([]);
  const [drop, setDrop] = useState<"idle" | "over">("idle");
  const current = useRef<Attachment[]>([]);
  current.current = attachments;

  const add = async (files: readonly FileLike[]): Promise<void> => {
    const result = await addFiles(files, current.current, caps, reader);
    current.current = [...current.current, ...result.accepted];
    setAttachments(current.current);
    setRejected(result.rejected);
  };
  useImperativeHandle(props.ref, () => ({ addFiles: add }));

  const reasons = {
    steer: reasonOf(caps?.steer), followUp: reasonOf(caps?.followUp), abort: reasonOf(caps?.abort), abortSubmission: reasonOf(caps?.abortSubmission),
  };
  const canSend = text.trim() !== "" || attachments.length > 0;

  const submit = (whenBusy: WhenBusy) => {
    if (caps === null || !canSend || reasons[whenBusy] !== null) return;
    props.onSubmit({ text: text.trimEnd(), attachments, whenBusy });
    setText("");
    current.current = [];
    setAttachments([]);
    setRejected([]);
  };
  const enterTarget = (): WhenBusy | null => {
    if (!busy) return "followUp";
    return reasons.followUp === null ? "followUp" : reasons.steer === null ? "steer" : null;
  };
  const note = (reason: string | null) => (reason === null ? null : <small data-reason>{reason}</small>);

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    const target = enterTarget();
    if (target !== null) submit(target);
  };
  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    const files = filesFromClipboard(e.clipboardData);
    if (files.length === 0) return;
    e.preventDefault();
    void add(files);
  };
  const onDrag = (e: DragEvent<HTMLFormElement>) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    setDrop("over");
  };
  const onDrop = (e: DragEvent<HTMLFormElement>) => {
    e.preventDefault();
    setDrop("idle");
    void add(filesFromDrop(e.dataTransfer));
  };

  return (
    <form aria-label="Composer" data-drop={drop} onSubmit={(e) => { e.preventDefault(); const t = enterTarget(); if (t !== null) submit(t); }}
      onDragEnter={onDrag} onDragOver={onDrag} onDragLeave={() => setDrop("idle")} onDrop={onDrop}>
      {queued.length > 0 && (
        <ul aria-label="Queued">
          {queued.map((q) => (
            <li key={q.clientSubmissionId} data-queued={q.clientSubmissionId}>
              {q.text}{" "}
              <button type="button" aria-label="Cancel queued message" disabled={reasons.abortSubmission !== null} title={reasons.abortSubmission ?? undefined}
                onClick={() => props.onCancelQueued(q.clientSubmissionId)}>Cancel</button>
            </li>
          ))}
        </ul>
      )}
      {attachments.length > 0 && (
        <ul aria-label="Attachments">
          {attachments.map((a) => (
            <li key={a.name} data-attachment={a.name} data-kind={a.kind}>
              {a.name}{" "}
              <button type="button" aria-label={`Remove ${a.name}`}
                onClick={() => { current.current = current.current.filter((x) => x !== a); setAttachments(current.current); }}>×</button>
            </li>
          ))}
        </ul>
      )}
      {rejected.length > 0 && (
        <ul role="alert" aria-label="Rejected files">
          {rejected.map((r, i) => <li key={`${r.name}:${i}`}>{r.name}: {r.message}</li>)}
        </ul>
      )}
      <textarea aria-label="Message" value={text} disabled={caps === null} onChange={(e) => setText(e.currentTarget.value)} onKeyDown={onKeyDown} onPaste={onPaste} />
      <input type="file" multiple aria-label="Attach files" disabled={caps === null}
        onChange={(e) => { const input = e.currentTarget; const files = Array.from(input.files ?? []); input.value = ""; void add(files); }} />
      {busy ? (
        <>
          <button type="button" data-when-busy="steer" disabled={reasons.steer !== null} title={reasons.steer ?? undefined} onClick={() => submit("steer")}>Steer</button>
          {note(reasons.steer)}
          <button type="button" data-when-busy="followUp" disabled={reasons.followUp !== null} title={reasons.followUp ?? undefined} onClick={() => submit("followUp")}>Follow up</button>
          {note(reasons.followUp)}
          <button type="button" data-action="abort" disabled={reasons.abort !== null} title={reasons.abort ?? undefined} onClick={() => props.onAbort()}>Abort</button>
          {note(reasons.abort)}
        </>
      ) : (
        <>
          <button type="submit" data-action="send" disabled={reasons.followUp !== null} title={reasons.followUp ?? undefined}>Send</button>
          {note(reasons.followUp)}
        </>
      )}
    </form>
  );
}
