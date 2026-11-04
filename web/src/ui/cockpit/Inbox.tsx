import type { ReactElement } from "react";
import type { CockpitAction, InboxModel } from "../../protocol/cockpit.ts";

export function Inbox(props: {
  inbox: InboxModel | null;
  onAction(action: CockpitAction): void;
  onSelectSession(sessionId: string): void;
}): ReactElement {
  const { inbox, onAction, onSelectSession } = props;
  if (inbox === null) {
    return (
      <aside aria-label="Inbox">
        <h2>Inbox</h2>
        <p>Loading inbox…</p>
      </aside>
    );
  }
  return (
    <aside aria-label="Inbox">
      <h2>{`Inbox (${inbox.openCount})`}</h2>
      {inbox.engineRunning ? null : <p role="status">Engine is not running</p>}
      {inbox.rows.length === 0 ? <p data-empty>Nothing needs you</p> : null}
      <ul>
        {inbox.rows.map((row) => {
          const [first = "", ...rest] = row.label.split("\n");
          return (
            <li key={row.id} data-kind={row.kind} data-urgency={row.urgency}>
              <span data-part="kind">{row.kind}</span> <strong>{first}</strong>
              {rest.map((line, i) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: static label lines
                <span key={i}> {line}</span>
              ))}
              {row.sessionId !== null ? (
                <button type="button" data-action="open-session" onClick={() => onSelectSession(row.sessionId as string)}>
                  Open
                </button>
              ) : null}
              {row.url !== null ? <a href={row.url}>Link</a> : null}
              {row.actions.map((a) => (
                <button key={a.label} type="button" onClick={() => onAction(a.action)}>
                  {a.label}
                </button>
              ))}
            </li>
          );
        })}
      </ul>
    </aside>
  );
}
