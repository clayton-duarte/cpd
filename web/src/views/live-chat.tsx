// The live Chat and the centre column (web/README.md "The chat surface (shadcn AI Elements)"):
// the folded transcript and a pinned send box, all through the injected LiveApi. It is the only
// thing the centre column renders. The rail selects the Session — the Chat no longer lists
// Sessions and no longer renders its `Session` <select> — and the Chat takes the given `session`
// (prop), subscribing to it exactly as before; with no Session it asks for one from the rail.
// Every folded `snapshot` frame replaces the view through the existing fold (rule 3).
//
// A turn is rendered whole, as the AI Elements Chat pattern composes it
// (`ui.shadcn.com/docs/ai-elements/base/chat`): the thread is a Message Scroller, a person's words
// are a Bubble at the end of the row, and an agent's turn shows every step in order — Reasoning
// (`base/reasoning`) and Tool (`base/tool`) rows built on Trace, the answer as markdown, and the
// tool output through the Code Block. A cpd notice is a Marker. The live frame is only the tail
// (a whole transcript is too large for one EventSource event), so older turns are read a page at a
// time and kept above it.
import { useEffect, useMemo, useState, type FormEvent, type KeyboardEvent, type ReactElement, type ReactNode } from "react";
import {
  BracesIcon,
  BrainIcon,
  CircleAlertIcon,
  FilePenIcon,
  FilePlusIcon,
  FileTextIcon,
  MessageCircleQuestionIcon,
  SquareTerminalIcon,
  WrenchIcon,
} from "lucide-react";
import type { LiveApi } from "../api/live";
import type { LiveSession } from "../api/live-server";
import { fold, type ChatMessage, type ChatView, type SnapshotFrame } from "../fold/fold";
import { Markdown } from "../components/markdown";
import { AskCard } from "../components/ask-card";
import { Bubble, BubbleContent } from "../components/ui/bubble";
import { Button } from "../components/ui/button";
import { CodeBlock, CodeBlockContent } from "../components/ui/code-block";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "../components/ui/empty";
import { InputGroup, InputGroupButton, InputGroupTextarea } from "../components/ui/input-group";
import { Marker, MarkerContent, MarkerIcon } from "../components/ui/marker";
import { Message, MessageContent } from "../components/ui/message";
import {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
} from "../components/ui/message-scroller";
import { Spinner } from "../components/ui/spinner";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "../components/ui/trace";
import type { TranscriptMessage, TranscriptPart, TranscriptToolPart } from "../transcript";

/** The live Chat: the folded transcript of the given Session and a send box. */
export interface LiveChatProps {
  /** The live API; absent when the probe failed — then the explicit no-adapter state shows. */
  api?: LiveApi;
  token?: string;
  /** The Session the rail selected; absent until a row is clicked. */
  session?: LiveSession;
}

/** The view before any session is subscribed; the first snapshot frame replaces it (fold rule 3). */
const EMPTY_VIEW: ChatView = { epoch: "", seq: 0, messages: [], cursor: { epoch: "", seq: 0 } };

/** How many older turns one "Load earlier messages" click asks for. */
const HISTORY_PAGE = 40;

/**
 * The fold's `ChatMessage` is the production wire shape; the dev adapter's snapshot carries its own
 * richer message through it untouched (fold rule 3 replaces the view with the snapshot array). This
 * is the one place that narrows back, and a message with no parts is a single text part.
 */
function asTranscript(message: ChatMessage): TranscriptMessage {
  const rich = message as ChatMessage & Partial<TranscriptMessage>;
  const parts: TranscriptPart[] =
    rich.parts ?? (message.text === "" ? [] : [{ kind: "text", text: message.text }]);
  return {
    id: message.id,
    role: rich.role ?? message.role,
    text: message.text,
    ...(rich.name === undefined ? {} : { name: rich.name }),
    ...(rich.delivery === undefined ? {} : { delivery: rich.delivery }),
    ...(parts.length === 0 ? {} : { parts }),
  };
}

/** The first line of a tool call's command, when it ran one. */
function commandOf(part: TranscriptToolPart): string | undefined {
  const args = part.args as { command?: unknown } | undefined;
  return typeof args?.command === "string" ? args.command.split("\n")[0] : undefined;
}

/** A path a tool touched, when the call names one. */
function pathOf(part: TranscriptToolPart): string | undefined {
  const args = part.args as { path?: unknown; file_path?: unknown } | undefined;
  const path = args?.path ?? args?.file_path;
  return typeof path === "string" ? path : undefined;
}

/** What a step line says it did: the tool's own name is the floor. */
function toolTitle(part: TranscriptToolPart): string {
  const command = commandOf(part);
  const path = pathOf(part);
  switch (part.name) {
    case "bash":
      return command === undefined ? "Ran a command" : `Ran ${command}`;
    case "read":
      return path === undefined ? "Read a file" : `Read ${path}`;
    case "edit":
      return path === undefined ? "Edited a file" : `Edited ${path}`;
    case "write":
      return path === undefined ? "Wrote a file" : `Wrote ${path}`;
    case "codemode":
      return "Ran a codemode script";
    case "ask_user_question":
      return "Asked you a question";
    case "cpd_plan_review":
      return "Asked you to review the Plan";
    default:
      return `Ran ${part.name}`;
  }
}

/** The icon of a step line, by the tool that ran. */
function toolIcon(part: TranscriptToolPart): ReactNode {
  switch (part.name) {
    case "bash":
      return <SquareTerminalIcon />;
    case "read":
      return <FileTextIcon />;
    case "edit":
      return <FilePenIcon />;
    case "write":
      return <FilePlusIcon />;
    case "codemode":
      return <BracesIcon />;
    case "ask_user_question":
      return <MessageCircleQuestionIcon />;
    default:
      return <WrenchIcon />;
  }
}

/** The step status a tool part maps to: a running call shimmers, a failed one is destructive. */
function toolStatus(part: TranscriptToolPart): "running" | "error" | undefined {
  if (part.state === "running") return "running";
  if (part.state === "error") return "error";
  return undefined;
}

/** The output of a step, marked up as the log or text it is. */
function StepOutput({ part }: { part: TranscriptToolPart }): ReactElement {
  const args = part.args === undefined ? undefined : JSON.stringify(part.args, null, 2);
  return (
    <div className="flex min-w-0 flex-col gap-2 py-1">
      {args !== undefined && (
        <CodeBlock size="sm">
          <CodeBlockContent wrap>{args}</CodeBlockContent>
        </CodeBlock>
      )}
      {part.output !== undefined && part.output !== "" && (
        <CodeBlock size="sm" variant={part.state === "error" ? "destructive" : "default"}>
          <CodeBlockContent wrap className="max-h-72">
            {part.output}
          </CodeBlockContent>
        </CodeBlock>
      )}
      {part.output === undefined && part.state === "running" && (
        <div className="flex items-center gap-2 text-muted-foreground">
          <Spinner className="size-3.5" />
          <span>Running…</span>
        </div>
      )}
    </div>
  );
}

/** The words of a thinking block, counted so a collapsed row still says how much is inside. */
function wordCount(text: string): string {
  const words = text.trim() === "" ? 0 : text.trim().split(/\s+/).length;
  return words === 1 ? "1 word" : `${words.toLocaleString()} words`;
}

/** One thinking block: the Reasoning pattern, collapsed behind its step line. */
function Reasoning({ text }: { text: string }): ReactElement {
  return (
    <Trace>
      <TraceCollapsible>
        <TraceCollapsibleTrigger>
          <TraceLine>
            <TraceIcon>
              <BrainIcon />
            </TraceIcon>
            <TraceTitle>Thought</TraceTitle>
            <TraceDescription>{wordCount(text)}</TraceDescription>
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="whitespace-pre-wrap">
          {text}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

/** One tool call: the Tool pattern, its arguments and its result inside. */
function ToolStep({ part }: { part: TranscriptToolPart }): ReactElement {
  // An ask is a conversation, not a command: it gets the question card, never a JSON blob.
  if (part.questions !== undefined) return <AskCard part={part} />;
  return (
    <Trace>
      <TraceCollapsible>
        <TraceCollapsibleTrigger>
          <TraceLine status={toolStatus(part)}>
            <TraceIcon>{toolIcon(part)}</TraceIcon>
            <TraceTitle>{toolTitle(part)}</TraceTitle>
            {part.durationMs !== undefined && (
              <TraceDescription>{(part.durationMs / 1000).toFixed(1)}s</TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent>
          <StepOutput part={part} />
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

/** One image of a turn, inline as the data URL pi stored. */
function Image({ part }: { part: Extract<TranscriptPart, { kind: "image" }> }): ReactElement {
  return (
    <img
      src={`data:${part.mime};base64,${part.data}`}
      alt="Attached image"
      className="max-w-full rounded-md border"
    />
  );
}

/** One turn: a person's words, an agent's whole chain of thought, or a cpd notice. */
function Turn({ message }: { message: TranscriptMessage }): ReactElement | null {
  const parts: TranscriptPart[] = message.parts ?? [{ kind: "text", text: message.text }];

  if (message.role === "system") {
    // A cpd notice (a session message, a Plan review request) is long and multi-line: the plain
    // Marker shape reads it left to right, where the separator variant would squeeze it.
    return (
      <Marker className="items-start border-s-2 border-border ps-2">
        <MarkerIcon className="mt-0.5">
          <CircleAlertIcon />
        </MarkerIcon>
        <MarkerContent className="max-w-full text-xs whitespace-pre-wrap">
          {message.text}
        </MarkerContent>
      </Marker>
    );
  }

  if (message.role === "user") {
    return (
      <Message align="end">
        <MessageContent>
          {/* A message cpd delivered (from the User, from another Session) names where it came
              from and why, so a turn nobody typed at this prompt is still honest about it. */}
          {message.delivery !== undefined && (
            <span className="text-muted-foreground text-xs" title={message.delivery.header}>
              {message.delivery.at}
              {message.delivery.reason === undefined ? null : ` \u00b7 ${message.delivery.reason}`}
            </span>
          )}
          {parts.map((part, index) =>
            part.kind === "image" ? (
              <Image key={index} part={part} />
            ) : part.kind === "text" ? (
              <Bubble key={index} align="end" variant="muted">
                <BubbleContent>
                  <Markdown>{part.text}</Markdown>
                </BubbleContent>
              </Bubble>
            ) : null
          )}
        </MessageContent>
      </Message>
    );
  }

  if (parts.length === 0) return null;

  return (
    <Message>
      <MessageContent>
        <div className="flex min-w-0 flex-col gap-2">
          {parts.map((part, index) => {
            switch (part.kind) {
              case "thinking":
                return <Reasoning key={index} text={part.text} />;
              case "tool":
                return <ToolStep key={index} part={part} />;
              case "image":
                return <Image key={index} part={part} />;
              case "text":
                return part.text.trim() === "" ? null : (
                  <Markdown key={index}>{part.text}</Markdown>
                );
            }
          })}
        </div>
      </MessageContent>
    </Message>
  );
}

/** Renders the live Chat: the chat alone is the centre column. */
export function LiveChat({ api, session }: LiveChatProps): ReactElement {
  const [view, setView] = useState<ChatView>(EMPTY_VIEW);
  const [text, setText] = useState("");
  // The live frame is only the tail, so older turns are read a page at a time and kept above it.
  const [older, setOlder] = useState<readonly TranscriptMessage[]>([]);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [noMore, setNoMore] = useState(false);
  const sessionId = session?.id;

  // Subscribe to the given Session; each snapshot frame folds onto the view (rule 3).
  useEffect(() => {
    setView(EMPTY_VIEW);
    setOlder([]);
    setNoMore(false);
    if (api === undefined || sessionId === undefined) return;
    const close = api.subscribe(sessionId, (frame: SnapshotFrame) => {
      setView((current) => fold(current, frame));
    });
    return close;
  }, [api, sessionId]);

  const tail = useMemo(
    () => view.messages.map(asTranscript).filter((message) => message.parts !== undefined),
    [view.messages],
  );

  // A frame replaces the tail wholesale, so a loaded page and the tail can overlap: dedupe by id.
  const messages = useMemo(() => {
    const seen = new Set(tail.map((message) => message.id));
    return [...older.filter((message) => !seen.has(message.id)), ...tail];
  }, [older, tail]);

  const loadEarlier = (): void => {
    if (api === undefined || sessionId === undefined || loadingOlder) return;
    const oldest = messages[0]?.id;
    setLoadingOlder(true);
    const page = api.history?.(sessionId, oldest, HISTORY_PAGE);
    if (page === undefined) {
      setLoadingOlder(false);
      return;
    }
    void page.then(
      (result) => {
        const loaded = result.messages.map((message) => asTranscript(message as ChatMessage));
        setOlder((current) => [...loaded, ...current]);
        setNoMore(!result.more);
        setLoadingOlder(false);
      },
      () => setLoadingOlder(false),
    );
  };

  const unavailable = api === undefined;
  const disabled = unavailable || session === undefined;

  const send = (): void => {
    const value = text.trim();
    if (api === undefined || sessionId === undefined || value === "") return;
    void api.send(sessionId, value);
    setText("");
  };

  const submit = (event: FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    send();
  };

  // Enter sends; Shift+Enter adds a line.
  const keyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      event.currentTarget.form?.requestSubmit();
    }
  };

  return (
    <MessageScrollerProvider autoScroll>
      <div className="flex h-dvh flex-col">
        {unavailable ? (
          <Empty className="flex-1">
            <EmptyHeader>
              <EmptyTitle>Live adapter off</EmptyTitle>
              <EmptyDescription>
                Start the dev server with CPD_WEB_LIVE=1 to read a real Session
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : session === undefined ? (
          <Empty className="flex-1">
            <EmptyHeader>
              <EmptyTitle>Pick a manager or a lead in the left rail to see its chat.</EmptyTitle>
            </EmptyHeader>
          </Empty>
        ) : (
          <MessageScroller className="min-h-0 flex-1">
            <MessageScrollerViewport>
              <MessageScrollerContent>
                {!noMore && (
                  <MessageScrollerItem className="min-w-0" messageId="load-earlier">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="mx-auto text-muted-foreground"
                      disabled={loadingOlder}
                      onClick={loadEarlier}
                    >
                      {loadingOlder ? <Spinner className="size-3.5" /> : null}
                      Load earlier messages
                    </Button>
                  </MessageScrollerItem>
                )}
                {messages.map((message) => (
                  <MessageScrollerItem
                    key={message.id}
                    messageId={message.id}
                    scrollAnchor={message.role === "user"}
                    className="min-w-0"
                  >
                    <Turn message={message} />
                  </MessageScrollerItem>
                ))}
              </MessageScrollerContent>
            </MessageScrollerViewport>
            <MessageScrollerButton />
          </MessageScroller>
        )}

        <form className="border-t p-2" onSubmit={submit}>
          <InputGroup>
            <InputGroupTextarea
              placeholder="Send a message"
              value={text}
              disabled={disabled}
              onChange={(event) => setText(event.target.value)}
              onKeyDown={keyDown}
            />
            <InputGroupButton type="submit" disabled={disabled}>
              Send
            </InputGroupButton>
          </InputGroup>
        </form>
      </div>
    </MessageScrollerProvider>
  );
}
