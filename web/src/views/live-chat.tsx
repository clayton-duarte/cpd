// The live Chat and the centre column (web/README.md "The chat surface (shadcn AI Elements)"):
// the folded transcript and a pinned send box, all through the injected LiveApi. It is the only
// thing the centre column renders. The rail selects the Session — the Chat no longer lists
// Sessions and no longer renders its `Session` <select> or its state/role badges — and the Chat
// takes the given `session` (prop), subscribing to it exactly as before; with no Session it asks
// for one from the rail. Every folded `snapshot` frame replaces the view through the existing
// fold (rule 3). Messages render markdown through the pinned `Markdown` wrapper (never bare
// text). With no live adapter the screen shows an explicit Empty state instead of a blank one.
import { useEffect, useState, type FormEvent, type KeyboardEvent, type ReactElement } from "react";
import type { LiveApi } from "../api/live";
import type { LiveSession } from "../api/live-server";
import { fold, type ChatMessage, type ChatView, type SnapshotFrame } from "../fold/fold";
import { Bubble, BubbleContent } from "../components/ui/bubble";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "../components/ui/empty";
import { InputGroup, InputGroupButton, InputGroupTextarea } from "../components/ui/input-group";
import { Markdown } from "../components/markdown";
import {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
} from "../components/ui/message-scroller";
import { Message, MessageContent } from "../components/ui/message";

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

/** One folded turn: the person's Bubble, the agent's Markdown (never a raw `{text}`). */
function MessageTurn({ message }: { message: ChatMessage }): ReactElement {
  if (message.role === "user") {
    return (
      <MessageScrollerItem messageId={message.id} scrollAnchor>
        <Message align="end">
          <MessageContent>
            <Bubble align="end" variant="muted">
              <BubbleContent>{message.text}</BubbleContent>
            </Bubble>
          </MessageContent>
        </Message>
      </MessageScrollerItem>
    );
  }
  return (
    <MessageScrollerItem messageId={message.id}>
      <Message align="start">
        <MessageContent>
          <Markdown>{message.text}</Markdown>
        </MessageContent>
      </Message>
    </MessageScrollerItem>
  );
}

/** Renders the live Chat: the chat alone is the centre column. */
export function LiveChat({ api, session }: LiveChatProps): ReactElement {
  const [view, setView] = useState<ChatView>(EMPTY_VIEW);
  const [text, setText] = useState("");
  const sessionId = session?.id;

  // Subscribe to the given Session; each snapshot frame folds onto the view (rule 3).
  useEffect(() => {
    setView(EMPTY_VIEW);
    if (api === undefined || sessionId === undefined) return;
    const close = api.subscribe(sessionId, (frame: SnapshotFrame) => {
      setView((current) => fold(current, frame));
    });
    return close;
  }, [api, sessionId]);

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
                {view.messages.map((message) => (
                  <MessageTurn key={message.id} message={message} />
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
