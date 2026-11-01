// The live Chat and the whole screen (web/README.md "The chat surface (shadcn AI Elements)",
// hs-d819): a session picker, the folded transcript and a pinned send box, all through the
// injected LiveApi. It is the only thing App renders — no app header, no nav and no
// Tree/Plan/Inbox/Logs panes. The transcript scrolls inside a Message Scroller viewport and the
// composer is a sibling below it, so it never scrolls away; each folded `snapshot` frame
// replaces the view through the existing fold (rule 3). Messages render markdown through the
// pinned `Markdown` wrapper (never bare text). With no live adapter the screen shows an explicit
// Empty state instead of a blank screen.
import { useEffect, useState, type FormEvent, type KeyboardEvent, type ReactElement } from "react";
import type { LiveApi } from "../api/live";
import type { LiveSession } from "../api/live-server";
import { fold, type ChatMessage, type ChatView, type SnapshotFrame } from "../fold/fold";
import { Badge } from "../components/ui/badge";
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

/** The live Chat: a session picker, the folded transcript, a send box. */
export interface LiveChatProps {
  /** The live API; absent when the probe failed — then the explicit no-adapter state shows. */
  api?: LiveApi;
  token?: string;
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

/** Renders the live Chat: the chat alone is the screen. */
export function LiveChat({ api }: LiveChatProps): ReactElement {
  const [sessions, setSessions] = useState<readonly LiveSession[]>([]);
  const [selected, setSelected] = useState<string | undefined>(undefined);
  const [view, setView] = useState<ChatView>(EMPTY_VIEW);
  const [text, setText] = useState("");

  // List the lead/manager Sessions once; the first is selected.
  useEffect(() => {
    if (api === undefined) return;
    let cancelled = false;
    void api.sessions().then((list) => {
      if (cancelled) return;
      setSessions(list);
      setSelected((current) => current ?? list[0]?.id);
    });
    return () => {
      cancelled = true;
    };
  }, [api]);

  // Subscribe to the selected Session; each snapshot frame folds onto the view (rule 3).
  useEffect(() => {
    if (api === undefined || selected === undefined) return;
    setView(EMPTY_VIEW);
    const close = api.subscribe(selected, (frame: SnapshotFrame) => {
      setView((current) => fold(current, frame));
    });
    return close;
  }, [api, selected]);

  const current = sessions.find((session) => session.id === selected);
  const unavailable = api === undefined;
  const disabled = unavailable || selected === undefined;

  const send = (): void => {
    const value = text.trim();
    if (api === undefined || selected === undefined || value === "") return;
    void api.send(selected, value);
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
        <div className="flex items-center gap-2 border-b p-2 text-sm">
          <label className="flex items-center gap-2">
            Session
            <select
              className="rounded border bg-background px-2 py-1"
              value={selected ?? ""}
              disabled={unavailable}
              onChange={(event) => setSelected(event.target.value)}
            >
              {sessions.map((session) => (
                <option key={session.id} value={session.id}>
                  {session.title ?? session.id}
                </option>
              ))}
            </select>
          </label>
          {current !== undefined ? <Badge variant="secondary">{current.state ?? "live"}</Badge> : null}
          {current !== undefined ? <Badge variant="outline">{current.role}</Badge> : null}
        </div>

        {unavailable ? (
          <Empty className="flex-1">
            <EmptyHeader>
              <EmptyTitle>Live adapter off</EmptyTitle>
              <EmptyDescription>
                Start the dev server with CPD_WEB_LIVE=1 to read a real Session
              </EmptyDescription>
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
