// The Chat cockpit view — the durable conversation, rendered from the fold
// (docs/research/web-client-api.md §5, §6; hs-d772, hs-d777). The view folds the host frames
// it is handed (the four fold rules, §5), renders the transcript as markdown, and carries a
// submit box that POSTs through the typed API. Approval `ui_request`s are answered by the
// app shell's dialog (`../app/App`); the Escalation area shows the count only (hs-d776).
import { useMemo, useState, type ReactElement } from "react";
import Markdown from "react-markdown";
import type { CpdApi } from "../api/mock";
import { submitPost } from "../api/actions";
import { fold, type ChatView, type EventsFrame, type SnapshotFrame } from "../fold/fold";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { Input } from "../components/ui/input";

/** A host frame the Chat view folds: one `events` commit or a whole-view `snapshot` (§5). */
export type ChatFrame = EventsFrame | SnapshotFrame;

/** The Chat view reads the host frames of one conversation. */
export interface ChatProps {
  /** The epoch the frames belong to (a restart is a new epoch, §5.3). */
  epoch: string;
  frames: readonly ChatFrame[];
  /** The typed API the submit box POSTs through (§4.4); absent when there is no socket. */
  api?: CpdApi;
  /** The token every request carries (§4.2); absent when there is no socket. */
  token?: string;
  /** The host state shown as the state pill (`host_state.state`, §1.1). */
  state?: string;
  /** The held Escalation count; the view shows the count only, never an answer UI (hs-d776). */
  escalations?: number;
}

/** Folds the handed frames in order onto a fresh view for `epoch` (rules 1-4, §5). */
function foldFrames(epoch: string, frames: readonly ChatFrame[]): ChatView {
  let view: ChatView = { epoch, seq: 0, messages: [], cursor: { epoch, seq: 0 } };
  for (const frame of frames) {
    view = fold(view, frame);
  }
  return view;
}

/** Renders the cockpit Chat from the folded frames (read-only transcript; A7, hs-d777). */
export function Chat({
  epoch,
  frames,
  api,
  token,
  state = "idle",
  escalations = 0,
}: ChatProps): ReactElement {
  const view = useMemo(() => foldFrames(epoch, frames), [epoch, frames]);
  const [draft, setDraft] = useState("");
  const canSubmit = api !== undefined && token !== undefined && token.length > 0;

  function submit(): void {
    if (!canSubmit || draft.trim().length === 0) return;
    api.post(submitPost(draft), { token });
    setDraft("");
  }

  return (
    <Card data-view="chat">
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>Chat</CardTitle>
        <div className="flex items-center gap-2">
          <Badge variant="secondary">{state}</Badge>
          <Badge variant="outline">escalations {escalations}</Badge>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {api === undefined ? (
          <p className="text-sm text-muted-foreground">
            No socket: showing the read-only transcript.
          </p>
        ) : null}

        <div className="flex flex-col gap-2">
          {view.messages.length === 0 ? (
            <p className="text-sm text-muted-foreground">No messages.</p>
          ) : (
            view.messages.map((message) => (
              <div key={message.id} className="rounded-md border p-2 text-sm">
                <div className="mb-1 flex items-center gap-2">
                  <Badge variant="outline">{message.role}</Badge>
                  {message.pending ? <span className="text-muted-foreground">pending</span> : null}
                </div>
                <Markdown>{message.text}</Markdown>
              </div>
            ))
          )}
        </div>

        {canSubmit ? (
          <form
            className="flex items-center gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
          >
            <Input
              aria-label="message"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              placeholder="Send a message"
            />
            <Button type="submit">Send</Button>
          </form>
        ) : null}
      </CardContent>
    </Card>
  );
}
