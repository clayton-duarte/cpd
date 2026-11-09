import { ArrowUpIcon, FileTextIcon, PlusIcon, SquareIcon, XIcon } from "lucide-react";
import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import type { ClipboardEvent, KeyboardEvent, ReactElement, ReactNode } from "react";
import { cn } from "../../lib/utils.ts";
import type {
  Attachment as AttachmentItem,
  Capabilities,
  QueuedSubmission,
  WhenBusy,
} from "../../protocol/domain.ts";
import {
  Attachment,
  AttachmentAction,
  AttachmentActions,
  AttachmentContent,
  AttachmentGroup,
  AttachmentMedia,
  AttachmentTitle,
} from "../components/attachment.tsx";
import { Button } from "../components/button.tsx";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupTextarea,
} from "../components/input-group.tsx";
import { clearDraft, loadDraft, saveDraft } from "./drafts.ts";
import { addFiles, filesFromClipboard } from "./attachments.ts";
import type {
  AttachmentRejection,
  FileLike,
  FileReaderBoundary,
} from "./attachments.ts";

export interface ComposerSubmit {
  readonly text: string;
  readonly attachments: readonly AttachmentItem[];
  readonly whenBusy: WhenBusy;
}
export interface ComposerHandle {
  addFiles(files: readonly FileLike[]): Promise<void>;
  focus(): void;
}

interface ComposerProps {
    capabilities: Capabilities | null;
    busy: boolean;
    queued: readonly QueuedSubmission[];
    reader: FileReaderBoundary;
    onSubmit(s: ComposerSubmit): void;
    onAbort(): void;
    onCancelQueued(clientSubmissionId: string): void;
    sessionId: string;
    settings?: ReactNode;
    archived?: boolean;
}

// Keyed by session so text and attachments never carry over to another session.
export const Composer = forwardRef<ComposerHandle, ComposerProps>(function Composer(props, ref): ReactElement {
  return <SessionComposer key={props.sessionId} {...props} ref={ref} />;
});

const SessionComposer = forwardRef<ComposerHandle, ComposerProps>(function SessionComposer(props, ref): ReactElement {
  const { capabilities, queued, reader, onSubmit, onCancelQueued } = props;
  const [text, setText] = useState(() => loadDraft(props.sessionId));
  const [attachments, setAttachments] = useState<AttachmentItem[]>([]);
  const [rejections, setRejections] = useState<AttachmentRejection[]>([]);
  const [dropping, setDropping] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const archived = props.archived ?? false;
  const canSend = capabilities !== null && !archived;
  const hasContent = text.trim() !== "" || attachments.length > 0;
  const canSubmit = canSend && hasContent;
  const stateRef = useRef({ attachments, capabilities, reader });
  useEffect(() => {
    stateRef.current = { attachments, capabilities, reader };
  });

  const handleFiles = async (files: readonly FileLike[]): Promise<void> => {
    if (files.length === 0) return;
    const { attachments: cur, capabilities: cap, reader: r } = stateRef.current;
    const res = await addFiles(files, cur, cap, r);
    if (res.accepted.length > 0) {
      setAttachments((prev) => [...prev, ...res.accepted]);
    }
    setRejections(res.rejected);
  };
  useImperativeHandle(ref, () => ({
    addFiles: handleFiles,
    focus: () => textareaRef.current?.focus(),
  }));

  const send = (): void => {
    if (!canSubmit) return;
    onSubmit({ text, attachments, whenBusy: "followUp" });
    clearDraft(props.sessionId);
    setText("");
    setAttachments([]);
    setRejections([]);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>): void => {
    if (e.nativeEvent.isComposing || e.key !== "Enter" || e.shiftKey) return;
    e.preventDefault();
    send();
  };

  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>): void => {
    const files = filesFromClipboard(e.clipboardData);
    if (files.length === 0) return;
    e.preventDefault();
    void handleFiles(files);
  };

  return (
    <div data-part="composer" className="mx-3 mb-2.5 flex shrink-0 flex-col gap-2">
      <input
        ref={fileInputRef}
        type="file"
        multiple
        aria-label="Attach files"
        disabled={!canSend}
        className="hidden"
        onChange={(e) => {
          const list = e.currentTarget.files;
          if (list && list.length > 0) void handleFiles(Array.from(list));
          e.currentTarget.value = "";
        }}
      />
      {rejections.length === 0 ? null : (
        <ul
          role="alert"
          data-part="rejections"
          className="m-0 flex list-none flex-col gap-1 p-0"
        >
          {rejections.map((r, i) => (
            <li
              key={i}
              data-rejection={r.reason}
              className="rounded-lg border border-destructive/50 bg-destructive/10 px-2.5 py-1.5 font-mono text-xs text-destructive"
            >
              {r.name}: {r.message}
            </li>
          ))}
        </ul>
      )}
      <InputGroup
        data-busy={props.busy ? "" : undefined}
        data-drop={dropping ? "over" : "idle"}
        onDragEnter={(e) => {
          if (Array.from(e.dataTransfer?.types ?? []).includes("Files")) setDropping(true);
        }}
        onDragLeave={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropping(false);
        }}
        onDrop={() => setDropping(false)}
        className={cn("rounded-3xl bg-card", props.busy && "prompt-border-pulse")}
      >
        {(queued.length > 0 || attachments.length > 0 || dropping) && (
          <InputGroupAddon
            align="block-start"
            className="flex-col items-stretch gap-2 px-3 pt-3 pb-0"
          >
            {dropping ? (
              <span data-part="drop-cue" className="font-mono text-xs text-muted-foreground">
                Drop to attach
              </span>
            ) : null}
            {queued.length === 0 ? null : (
              <ul
                aria-label="Queued submissions"
                data-part="queue"
                className="m-0 flex list-none flex-wrap gap-1.5 p-0"
              >
                {queued.map((q) => (
                  <li
                    key={q.clientSubmissionId}
                    data-queued-chip={q.clientSubmissionId}
                    className="inline-flex max-w-[360px] items-center gap-1.5 rounded-md border bg-background px-2 py-0.5 text-xs text-foreground"
                  >
                    <span className="font-mono text-muted-foreground">
                      queued
                    </span>
                    <span data-part="text" className="truncate">
                      {q.text}
                    </span>
                    {capabilities?.abortSubmission?.supported ? (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-xs"
                        aria-label={`Cancel queued: ${q.text}`}
                        data-action="cancel-queued"
                        onClick={() => onCancelQueued(q.clientSubmissionId)}
                      >
                        ×
                      </Button>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
            {attachments.length === 0 ? null : (
              <AttachmentGroup
                aria-label="Staged attachments"
                data-part="attachments"
              >
                {attachments.map((a, i) => (
                  <Attachment key={i} size="sm" data-attachment={a.kind}>
                    <AttachmentMedia
                      data-part="thumb"
                      variant={a.kind === "image" ? "image" : "icon"}
                    >
                      {a.kind === "image" ? (
                        <img
                          alt=""
                          src={`data:${a.mimeType};base64,${a.dataBase64}`}
                        />
                      ) : (
                        <FileTextIcon />
                      )}
                    </AttachmentMedia>
                    <AttachmentContent>
                      <AttachmentTitle data-part="name" className="font-mono">
                        {a.name}
                      </AttachmentTitle>
                    </AttachmentContent>
                    <AttachmentActions>
                      <AttachmentAction
                        aria-label={`Remove ${a.name}`}
                        data-action="remove-attachment"
                        onClick={() =>
                          setAttachments((cur) =>
                            cur.filter((_, k) => k !== i),
                          )
                        }
                      >
                        <XIcon />
                      </AttachmentAction>
                    </AttachmentActions>
                  </Attachment>
                ))}
              </AttachmentGroup>
            )}
          </InputGroupAddon>
        )}
        <InputGroupTextarea
          ref={textareaRef}
          aria-label="Message"
          disabled={!canSend}
          rows={1}
          placeholder={archived ? "Session is archived - unarchive to send messages" : "Ask anything… (Enter to send, Shift+Enter for newline)"}
          className="min-h-10 px-4 py-3 text-sm leading-relaxed"
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            saveDraft(props.sessionId, e.target.value);
          }}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
        />
        <InputGroupAddon
          align="block-end"
          className="justify-between px-3 pb-2.5 pt-0"
        >
          <div className="flex items-center gap-1.5">
            <InputGroupButton
              type="button"
              variant="outline"
              size="icon-sm"
              className="rounded-full"
              aria-label="Attach files"
              disabled={!canSend}
              onClick={() => fileInputRef.current?.click()}
            >
              <PlusIcon />
            </InputGroupButton>
          </div>
          <div data-part="actions" className="flex items-center gap-1.5">
            {props.settings}
            {props.busy && capabilities?.abort.supported ? (
              <InputGroupButton
                type="button"
                variant="ghost"
                size="icon-sm"
                data-action="abort"
                aria-label="Stop"
                className="rounded-full text-destructive"
                onClick={props.onAbort}
              >
                <SquareIcon />
                <span className="sr-only">Stop</span>
              </InputGroupButton>
            ) : null}
            <InputGroupButton
              type="button"
              variant="default"
              size="icon-sm"
              data-action="send"
              aria-label="Send"
              disabled={!canSubmit}
              className="rounded-full"
              onClick={send}
            >
              <ArrowUpIcon />
              <span className="sr-only">Send</span>
            </InputGroupButton>
          </div>
        </InputGroupAddon>
      </InputGroup>
    </div>
  );
});
