// The live Chat view (web/README.md "The UI wiring", hs-d819): a session picker, the folded
// transcript and a send box, all through the injected LiveApi. The folded transcript enters the
// existing fold through a `snapshot` frame (fold rule 3), so the Chat rendering engine is
// unchanged. The transcript and the prompt are the shadcn AI Elements conversation composition
// (`Conversation`, `Message`, the `prompt-input` entry's provider/textarea/submit), copied in
// under web/src/components; the `PromptInput` attachment wrapper is the one piece left out, so
// no hidden file input precedes the textarea.
import { useEffect, useState, type FormEvent, type ReactElement } from "react";
import type { LiveApi } from "../api/live";
import type { LiveSession } from "../api/live-server";
import { fold, type ChatView, type SnapshotFrame } from "../fold/fold";
import { Badge } from "../components/ui/badge";
import { InputGroup } from "../components/ui/input-group";
import {
  Conversation,
  ConversationContent,
  ConversationScrollButton,
} from "../components/ai-elements/conversation";
import { Message, MessageContent } from "../components/ai-elements/message";
import {
  PromptInputProvider,
  PromptInputSubmit,
  PromptInputTextarea,
  usePromptInputController,
} from "../components/ai-elements/prompt-input";

/** The live Chat: a session picker, the folded transcript, a send box. */
export interface LiveChatProps {
  /** The live API; absent when the probe failed — then the mock's Chat renders instead. */
  api?: LiveApi;
  token?: string;
}

/** The view before any session is subscribed; the first snapshot frame replaces it (fold rule 3). */
const EMPTY_VIEW: ChatView = { epoch: "", seq: 0, messages: [], cursor: { epoch: "", seq: 0 } };

/** The send box's lifecycle, shown through the submit button's status. */
type SendStatus = "idle" | "sending" | "error";

/** Reads the provider's text, sends it through the live API and clears the box. */
function LiveComposer({
  disabled,
  onSubmit,
}: {
  disabled: boolean;
  onSubmit: (text: string) => void;
}): ReactElement {
  const controller = usePromptInputController();

  const submit = (event: FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    const text = controller.textInput.value.trim();
    if (disabled || text === "") return;
    onSubmit(text);
    controller.textInput.clear();
  };

  return (
    <form className="border-t p-2" onSubmit={submit}>
      <InputGroup>
        <PromptInputTextarea placeholder="Send a message" />
        <PromptInputSubmit disabled={disabled} />
      </InputGroup>
    </form>
  );
}

/** Renders the live Chat. */
export function LiveChat({ api }: LiveChatProps): ReactElement {
  const [sessions, setSessions] = useState<readonly LiveSession[]>([]);
  const [selected, setSelected] = useState<string | undefined>(undefined);
  const [view, setView] = useState<ChatView>(EMPTY_VIEW);
  const [status, setStatus] = useState<SendStatus>("idle");

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

  const send = (text: string): void => {
    if (api === undefined || selected === undefined) return;
    setStatus("sending");
    void api.send(selected, text).then(
      () => setStatus("idle"),
      () => setStatus("error"),
    );
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-2 border-b p-2 text-sm">
        <label className="flex items-center gap-2">
          Session
          <select
            className="rounded border bg-background px-2 py-1"
            value={selected ?? ""}
            onChange={(event) => setSelected(event.target.value)}
          >
            {sessions.map((session) => (
              <option key={session.id} value={session.id}>
                {session.title ?? session.id}
              </option>
            ))}
          </select>
        </label>
        <Badge variant="secondary">{current?.state ?? "live"}</Badge>
        {current !== undefined ? <Badge variant="outline">{current.role}</Badge> : null}
      </header>

      <Conversation className="min-h-0 flex-1">
        <ConversationContent>
          {view.messages.map((message) => (
            <Message key={message.id} from={message.role}>
              <MessageContent>{message.text}</MessageContent>
            </Message>
          ))}
        </ConversationContent>
        <ConversationScrollButton />
      </Conversation>

      <PromptInputProvider>
        <LiveComposer
          disabled={api === undefined || selected === undefined || status === "sending"}
          onSubmit={send}
        />
      </PromptInputProvider>
    </div>
  );
}
