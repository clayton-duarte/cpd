// The live Chat view (web/README.md "The UI wiring", hs-d819): a session picker, the folded
// transcript and a send box, all through the injected LiveApi. The folded transcript enters the
// existing fold through a `snapshot` frame (fold rule 3), so the Chat rendering engine is
// unchanged.
import { useEffect, useState, type FormEvent, type ReactElement } from "react";
import type { LiveApi } from "../api/live";
import type { LiveSession } from "../api/live-server";
import { fold, type ChatView, type SnapshotFrame } from "../fold/fold";

/** The live Chat: a session picker, the folded transcript, a send box. */
export interface LiveChatProps {
  /** The live API; absent when the probe failed — then the mock's Chat renders instead. */
  api?: LiveApi;
  token?: string;
}

/** The view before any session is subscribed; the first snapshot frame replaces it (fold rule 3). */
const EMPTY_VIEW: ChatView = { epoch: "", seq: 0, messages: [], cursor: { epoch: "", seq: 0 } };

/** Renders the live Chat. */
export function LiveChat({ api }: LiveChatProps): ReactElement {
  const [sessions, setSessions] = useState<readonly LiveSession[]>([]);
  const [selected, setSelected] = useState<string | undefined>(undefined);
  const [view, setView] = useState<ChatView>(EMPTY_VIEW);
  const [draft, setDraft] = useState("");

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

  const submit = (event: FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    const text = draft.trim();
    if (api === undefined || selected === undefined || text === "") return;
    void api.send(selected, text);
    setDraft("");
  };

  return (
    <div className="flex h-full flex-col">
      <label className="p-2 text-sm">
        Session
        <select
          className="ml-2 rounded border px-2 py-1"
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

      <ul className="flex-1 overflow-auto p-2">
        {view.messages.map((message) => (
          <li key={message.id} className="px-2 py-1">
            {message.text}
          </li>
        ))}
      </ul>

      <form className="flex gap-2 border-t p-2" onSubmit={submit}>
        <input
          className="flex-1 rounded border px-2 py-1"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="Send a message"
        />
        <button type="submit">Send</button>
      </form>
    </div>
  );
}
