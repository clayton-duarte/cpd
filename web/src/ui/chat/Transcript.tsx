import {
  BrainIcon,
  CheckIcon,
  CopyIcon,
  FileTextIcon,
  ImageIcon,
  MessageSquareIcon,
} from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { JSX } from "react";
import type {
  QueuedSubmission,
  ToolItem,
  TranscriptAttachment,
  TranscriptItem,
} from "../../protocol/domain.ts";
import type { PendingSubmission } from "../../state/store.ts";
import {
  Attachment,
  AttachmentContent,
  AttachmentGroup,
  AttachmentMedia,
  AttachmentTitle,
} from "../components/attachment.tsx";
import { Bubble, BubbleContent } from "../components/bubble.tsx";
import { Button } from "../components/button.tsx";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "../components/empty.tsx";
import { Marker, MarkerContent, MarkerIcon } from "../components/marker.tsx";
import {
  Message,
  MessageContent,
  MessageFooter,
} from "../components/message.tsx";
import {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
  useMessageScroller,
  useMessageScrollerScrollable,
} from "../components/message-scroller.tsx";
import { Spinner } from "../components/spinner.tsx";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "../components/trace.tsx";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "../components/tooltip.tsx";
import { glyphClass } from "../dango-status.ts";
import { CompactionRow, FoldRow, formatClock } from "./Folds.tsx";
import { Markdown } from "./Markdown.tsx";
import { groupRows } from "./rows.ts";
import type { Row } from "./rows.ts";
import { ToolGroup } from "./ToolGroup.tsx";
import { OrphanRow, ToolRow } from "./ToolRow.tsx";
import type { SizeObserver } from "./virtual.ts";

const SCROLL_EDGE_THRESHOLD = 160;

function ReasoningBlock(props: {
  text: string;
  time: string | null;
  defaultOpen: boolean;
  running: boolean;
}): JSX.Element {
  const [open, setOpen] = useState<boolean | undefined>(undefined);
  useEffect(() => {
    setOpen(undefined);
  }, [props.defaultOpen]);

  const hasText = props.text.trim().length > 0;
  const isOpen = open ?? (props.running || props.defaultOpen);

  const line = (
    <TraceLine status={props.running ? "running" : undefined}>
      <TraceIcon>{props.running ? <Spinner /> : <BrainIcon />}</TraceIcon>
      <TraceTitle data-part="label">
        {props.running ? "Thinking" : "Thought"}
      </TraceTitle>
      {props.time === null ? null : (
        <TraceDescription data-part="meta" className="font-mono text-xs">
          {props.time}
        </TraceDescription>
      )}
    </TraceLine>
  );

  if (!hasText) {
    return <Trace data-kind="thinking">{line}</Trace>;
  }

  return (
    <Trace data-kind="thinking">
      <TraceCollapsible open={isOpen} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger>{line}</TraceCollapsibleTrigger>
        <TraceCollapsibleContent
          data-part="thinking-body"
          className="whitespace-pre-wrap"
        >
          <Markdown text={props.text} />
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

function ChainOfThoughtRow(props: {
  row: Extract<Row, { kind: "chain" }>;
  thinking: "folded" | "expanded";
  tools: "grouped" | "expanded";
}): JSX.Element {
  const { row, thinking, tools } = props;
  const defaultOpen = thinking === "expanded" || tools === "expanded";
  const [open, setOpen] = useState<boolean | undefined>(undefined);
  useEffect(() => {
    setOpen(undefined);
  }, [defaultOpen]);

  const isOpen = open ?? defaultOpen;
  const clock = formatClock(row.at);
  const parts: string[] = [
    `${row.stepCount} ${row.stepCount === 1 ? "step" : "steps"}`,
  ];
  if (row.failedCount > 0) parts.push(`${row.failedCount} failed`);
  if (clock !== null) parts.push(clock);

  return (
    <Trace data-kind="chain-of-thought">
      <TraceCollapsible open={isOpen} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger>
          <TraceLine
            status={
              row.running
                ? "running"
                : row.failedCount > 0
                  ? "error"
                  : undefined
            }
          >
            <TraceIcon>{row.running ? <Spinner /> : <BrainIcon />}</TraceIcon>
            <TraceTitle data-part="label">Chain of Thought</TraceTitle>
            <TraceDescription data-part="meta" className="font-mono text-xs">
              {parts.join(" · ")}
            </TraceDescription>
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent
          data-part="chain-body"
          className="flex flex-col gap-2"
        >
          {row.steps.map((step) => {
            if (step.kind === "thinking") {
              return (
                <ReasoningBlock
                  key={step.key}
                  text={step.text}
                  time={formatClock(step.at)}
                  defaultOpen={thinking === "expanded"}
                  running={step.running}
                />
              );
            }
            if (step.kind === "tool") {
              return (
                <Trace key={step.key} data-part="tool-rail">
                  <ToolRow tool={step.tool} />
                </Trace>
              );
            }
            return (
              <ToolGroup key={step.key} tools={step.tools} mode={tools} />
            );
          })}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

function UserAttachments(props: {
  attachments: readonly TranscriptAttachment[];
}): JSX.Element | null {
  if (props.attachments.length === 0) return null;
  return (
    <AttachmentGroup data-part="attachments" className="justify-end">
      {props.attachments.map((a, i) =>
        a.kind === "image" && a.dataBase64 !== undefined ? (
          <div
            key={i}
            data-attachment="image"
            className="overflow-hidden rounded-xl border bg-card shadow-xs"
          >
            <img
              alt="Attached screenshot"
              data-part="thumb"
              src={`data:${a.mimeType};base64,${a.dataBase64}`}
              className="block max-h-32 w-auto max-w-52 object-cover"
            />
          </div>
        ) : (
          <Attachment key={i} size="sm" data-attachment={a.kind}>
            <AttachmentMedia data-part="thumb" variant="icon">
              {a.kind === "image" ? <ImageIcon /> : <FileTextIcon />}
            </AttachmentMedia>
            <AttachmentContent>
              <AttachmentTitle data-part="name" className="font-mono">
                {a.kind === "image" ? a.mimeType : a.name}
              </AttachmentTitle>
            </AttachmentContent>
          </Attachment>
        ),
      )}
    </AttachmentGroup>
  );
}

function CopyTurnButton(props: { value: string }): JSX.Element {
  const [copied, setCopied] = useState(false);

  const onCopy = () => {
    setCopied(true);
    try {
      void navigator.clipboard?.writeText(props.value);
    } catch {
      // ignore
    }
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            data-action="copy-turn"
            aria-label={copied ? "Copied" : "Copy message"}
            onClick={onCopy}
            className="size-5 text-muted-foreground hover:text-foreground"
          >
            {copied ? (
              <CheckIcon className="size-3" />
            ) : (
              <CopyIcon className="size-3" />
            )}
          </Button>
        </TooltipTrigger>
        <TooltipContent side="bottom">
          {copied ? "Copied" : "Copy"}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

const Item = memo(function Item(props: {
  item: Exclude<TranscriptItem, ToolItem>;
  sessionKey: string | undefined;
  thinking: "folded" | "expanded";
}): JSX.Element {
  const { item, thinking } = props;
  switch (item.kind) {
    case "user": {
      const clock = formatClock(item.at);
      return (
        <Message align="end" data-kind="user">
          <MessageContent>
            <UserAttachments attachments={item.attachments} />
            <Bubble
              align="end"
              variant="muted"
              data-part="bubble"
              className="max-w-full min-w-0 overflow-hidden"
            >
              <BubbleContent
                data-part="text"
                className="min-w-0 overflow-hidden break-words whitespace-pre-wrap"
              >
                {item.text}
              </BubbleContent>
            </Bubble>
            <MessageFooter data-part="meta" className="gap-1.5">
              <span>{clock === null ? "You" : `You · ${clock}`}</span>
              {item.text !== "" ? <CopyTurnButton value={item.text} /> : null}
            </MessageFooter>
          </MessageContent>
        </Message>
      );
    }
    case "assistant": {
      const clock = formatClock(item.at);
      let lastText = -1;
      const textParts: string[] = [];
      item.blocks.forEach((b, i) => {
        if (b.type === "text") {
          lastText = i;
          textParts.push(b.text);
        }
      });
      const fullText = textParts.join("\n\n");
      return (
        <Message data-kind="assistant">
          <MessageContent>
            {item.blocks.map((b, i) =>
              b.type === "text" ? (
                <Bubble
                  key={i}
                  variant="muted"
                  className="max-w-full min-w-0 overflow-hidden"
                >
                  <BubbleContent
                    data-part="card"
                    className="w-full min-w-0 overflow-hidden break-words"
                  >
                    <Markdown text={b.text} />
                  </BubbleContent>
                </Bubble>
              ) : (
                <ReasoningBlock
                  key={i}
                  text={b.text}
                  time={clock}
                  defaultOpen={thinking === "expanded"}
                  running={
                    item.streaming === true && i === item.blocks.length - 1
                  }
                />
              ),
            )}
            {item.streaming ? (
              <Marker role="status" data-part="working">
                <MarkerIcon>
                  <Spinner />
                </MarkerIcon>
                <MarkerContent className="shimmer">Working…</MarkerContent>
              </Marker>
            ) : null}
            {lastText >= 0 ? (
              <MessageFooter className="gap-2">
                {clock !== null ? <span>{clock}</span> : null}
                {fullText !== "" ? <CopyTurnButton value={fullText} /> : null}
              </MessageFooter>
            ) : null}
          </MessageContent>
        </Message>
      );
    }
    case "orphanResult":
      return <OrphanRow item={item} />;
    case "fold":
      return <FoldRow item={item} />;
    case "compaction":
      return <CompactionRow item={item} />;
  }
});

function RowBody(props: {
  row: Row;
  sessionKey: string | undefined;
  thinking: "folded" | "expanded";
  tools: "grouped" | "expanded";
}): JSX.Element {
  const { row } = props;
  if (row.kind === "chain") {
    return (
      <ChainOfThoughtRow
        row={row}
        thinking={props.thinking}
        tools={props.tools}
      />
    );
  }
  if (row.kind === "item") {
    if (row.item.kind === "tool") {
      return (
        <Trace data-part="tool-rail">
          <ToolRow tool={row.item} />
        </Trace>
      );
    }
    return (
      <Item
        item={row.item}
        sessionKey={props.sessionKey}
        thinking={props.thinking}
      />
    );
  }
  return <ToolGroup tools={row.tools} mode={props.tools} />;
}

function TranscriptAutoScroll(props: {
  tailEpoch: number;
  items: readonly TranscriptItem[];
  queuedCount: number;
  pendingCount: number;
}): null {
  const { scrollToEnd } = useMessageScroller();
  const scrollable = useMessageScrollerScrollable();
  const epochRef = useRef(props.tailEpoch);
  const pinnedRef = useRef(true);

  useEffect(() => {
    pinnedRef.current = !scrollable.end;
  }, [scrollable.end]);

  useEffect(() => {
    if (epochRef.current !== props.tailEpoch) {
      epochRef.current = props.tailEpoch;
      pinnedRef.current = true;
      scrollToEnd({ behavior: "auto" });
      return;
    }
    if (pinnedRef.current) {
      scrollToEnd({ behavior: "smooth" });
    }
  }, [
    props.tailEpoch,
    props.items,
    props.queuedCount,
    props.pendingCount,
    scrollToEnd,
  ]);

  return null;
}

export interface TranscriptProps {
  items: readonly TranscriptItem[];
  hasEarlier: boolean;
  loadingEarlier?: boolean;
  onLoadEarlier(): void;
  tailEpoch: number;
  queued: readonly QueuedSubmission[];
  pending: readonly PendingSubmission[];
  observer?: SizeObserver;
  sessionKey?: string;
  thinking?: "folded" | "expanded";
  tools?: "grouped" | "expanded";
}

export function Transcript(props: TranscriptProps): JSX.Element {
  const rows = useMemo(() => groupRows(props.items), [props.items]);
  const empty =
    rows.length === 0 && props.queued.length === 0 && props.pending.length === 0;

  return (
    <MessageScrollerProvider
      scrollEdgeThreshold={SCROLL_EDGE_THRESHOLD}
      defaultScrollPosition="end"
    >
      <TranscriptAutoScroll
        tailEpoch={props.tailEpoch}
        items={props.items}
        queuedCount={props.queued.length}
        pendingCount={props.pending.length}
      />
      <MessageScroller
        data-part="transcript"
        className="flex min-h-0 flex-1 flex-col overflow-hidden"
      >
        <MessageScrollerViewport
          aria-label="Conversation transcript"
          data-part="viewport"
          className="flex-1 overflow-x-hidden px-4 py-4"
        >
          <MessageScrollerContent className="mx-auto flex w-full max-w-4xl min-w-0 flex-col gap-3">
            {props.hasEarlier ? (
              <div className="flex justify-center py-1">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  data-action="load-earlier"
                  disabled={props.loadingEarlier}
                  onClick={() => props.onLoadEarlier()}
                >
                  {props.loadingEarlier ? "Loading…" : "Load earlier"}
                </Button>
              </div>
            ) : null}
            {empty ? (
              <Empty
                data-empty
                className="my-12 border-none py-12 text-muted-foreground"
              >
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <MessageSquareIcon />
                  </EmptyMedia>
                  <EmptyTitle>No messages yet</EmptyTitle>
                  <EmptyDescription>
                    Send a message below to start the conversation.
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : null}
            {rows.map((row) => {
              const isUser = row.kind === "item" && row.item.kind === "user";
              return (
                <MessageScrollerItem
                  key={row.key}
                  data-row={row.key}
                  className="flex w-full min-w-0 flex-col"
                >
                  <div
                    className={
                      isUser
                        ? "ml-auto w-full max-w-[85%] min-w-0 overflow-hidden"
                        : "mr-auto w-full max-w-[85%] min-w-0 overflow-hidden"
                    }
                  >
                    <RowBody
                      row={row}
                      sessionKey={props.sessionKey}
                      thinking={props.thinking ?? "folded"}
                      tools={props.tools ?? "grouped"}
                    />
                  </div>
                </MessageScrollerItem>
              );
            })}
            {props.queued.length === 0 && props.pending.length === 0 ? null : (
              <ul
                aria-label="Queued messages"
                className="m-0 ml-auto flex w-full max-w-[85%] list-none flex-col gap-2 p-0"
              >
                {props.queued.map((q) => (
                  <li
                    key={q.clientSubmissionId}
                    data-queued={q.clientSubmissionId}
                  >
                    <Message align="end">
                      <MessageContent>
                        <Bubble
                          align="end"
                          variant="outline"
                          data-part="bubble"
                          className="max-w-full"
                        >
                          <BubbleContent
                            data-part="text"
                            className="border-dashed whitespace-pre-wrap"
                          >
                            {q.text}
                          </BubbleContent>
                        </Bubble>
                        <MessageFooter
                          data-part="queued-status"
                          className="font-mono"
                        >
                          queued
                        </MessageFooter>
                      </MessageContent>
                    </Message>
                  </li>
                ))}
                {props.pending.map((p) => {
                  const clock = formatClock(p.at);
                  const pendingAttachments: TranscriptAttachment[] = (
                    p.images ?? []
                  ).map((im) => ({
                    kind: "image",
                    mimeType: im.mimeType,
                    dataBase64: im.dataBase64,
                  }));
                  return (
                    <li key={p.clientSubmissionId} data-pending={p.state}>
                      <Message align="end">
                        <MessageContent>
                          <UserAttachments attachments={pendingAttachments} />
                          <Bubble
                            align="end"
                            variant={
                              p.state === "failed" ? "destructive" : "muted"
                            }
                            data-part="bubble"
                            className="max-w-full min-w-0 overflow-hidden"
                          >
                            <BubbleContent
                              data-part="text"
                              className="min-w-0 overflow-hidden break-words whitespace-pre-wrap"
                            >
                              {p.text}
                            </BubbleContent>
                          </Bubble>
                          <MessageFooter className="gap-1.5">
                            {p.state === "failed" ? (
                              <span
                                data-part="pending-status"
                                data-state="failed"
                                role="alert"
                                className="flex items-center gap-1.5 font-mono text-destructive"
                              >
                                <span
                                  aria-hidden="true"
                                  className={glyphClass.blocked}
                                />
                                {`failed: ${p.error?.message ?? "unknown error"}`}
                              </span>
                            ) : (
                              <>
                                <span
                                  data-part="pending-status"
                                  data-state={p.state}
                                >
                                  {clock === null ? "You" : `You · ${clock}`}
                                </span>
                                {p.text !== "" ? (
                                  <CopyTurnButton value={p.text} />
                                ) : null}
                              </>
                            )}
                          </MessageFooter>
                        </MessageContent>
                      </Message>
                    </li>
                  );
                })}
              </ul>
            )}
          </MessageScrollerContent>
        </MessageScrollerViewport>
        <MessageScrollerButton data-action="jump-to-bottom" />
      </MessageScroller>
    </MessageScrollerProvider>
  );
}
