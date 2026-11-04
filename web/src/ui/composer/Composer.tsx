import { useImperativeHandle, useRef, useState } from "react";
import { Paperclip } from "lucide-react";
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
  const fileInput = useRef<HTMLInputElement>(null);
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
    const target = busy && e.altKey ? "followUp" : busy ? (reasons.steer === null ? "steer" : enterTarget()) : enterTarget();
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
    <form aria-label="Composer" data-drop={drop} className="rounded-lg border bg-card" onSubmit={(e) => { e.preventDefault(); const t = enterTarget(); if (t !== null) submit(t); }}
      onDragEnter={onDrag} onDragOver={onDrag} onDragLeave={() => setDrop("idle")} onDrop={onDrop}>
      {queued.length > 0 && (
        <ul aria-label="Queued" data-part="queued-bar" className="flex flex-wrap gap-1.5 border-b px-3 py-2">
          {queued.map((q) => (
            <li key={q.clientSubmissionId} data-queued={q.clientSubmissionId} data-when-busy={q.whenBusy}
              className="flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-xs">
              <span data-part="queued-kind">{q.whenBusy === "steer" ? "steer queued" : "follow-up queued"}</span>
              <span data-part="queued-text" className="truncate">{q.text}</span>
              <button type="button" aria-label="Cancel queued message" disabled={reasons.abortSubmission !== null} title={reasons.abortSubmission ?? undefined}
                onClick={() => props.onCancelQueued(q.clientSubmissionId)}>×</button>
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
      <textarea aria-label="Message" value={text} disabled={caps === null} rows={2} className="w-full resize-none bg-transparent px-3 py-2 outline-none"
        onChange={(e) => setText(e.currentTarget.value)} onKeyDown={onKeyDown} onPaste={onPaste} />
      <input ref={fileInput} type="file" multiple hidden aria-label="Attach files" disabled={caps === null}
        onChange={(e) => { const input = e.currentTarget; const files = Array.from(input.files ?? []); input.value = ""; void add(files); }} />
      <div data-part="action-row" className="flex items-center gap-2 px-2 pb-2">
        <button type="button" data-action="attach" aria-label="Choose files to attach" disabled={caps === null}
          onClick={() => fileInput.current?.click()}><Paperclip aria-hidden="true" size={16} /></button>
        {props.settings}
        <span className="ml-auto flex items-center gap-2">
          {busy ? (
            <>
              <div role="group" data-part="send-split" aria-label="Send while the agent is running" className="inline-flex">
                <button type="button" data-when-busy="steer" disabled={reasons.steer !== null} title={reasons.steer ?? undefined} onClick={() => submit("steer")}>Steer<kbd>⏎</kbd></button>
                <button type="button" data-when-busy="followUp" disabled={reasons.followUp !== null} title={reasons.followUp ?? undefined} onClick={() => submit("followUp")}>Follow-up<kbd>⌥⏎</kbd></button>
              </div>
              <button type="button" data-action="abort" disabled={reasons.abort !== null} title={reasons.abort ?? undefined} onClick={() => props.onAbort()}>Stop<kbd>⌃C</kbd></button>
            </>
          ) : (
            <button type="submit" data-action="send" disabled={reasons.followUp !== null} title={reasons.followUp ?? undefined}>Send<kbd>⏎</kbd></button>
          )}
        </span>
      </div>
      {busy ? <>{note(reasons.steer)}{note(reasons.followUp)}{note(reasons.abort)}</> : note(reasons.followUp)}
    </form>
  );
}
