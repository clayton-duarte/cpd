import { useEffect } from "react";
import type { JSX } from "react";
import { Badge } from "../components/badge.tsx";
import type { Completion, Runner, SessionRole } from "../../protocol/domain.ts";

export interface ChatHeaderProps {
  readonly sessionKey: string;
  readonly title: string | null;
  readonly role?: SessionRole;
  readonly runner?: Runner;
  readonly completion?: Completion | null;
}

const LABEL_TEXT: Readonly<Record<Completion["label"], string>> = {
  safe_to_archive: "Safe to archive",
  unanswered_ask: "Unanswered ask",
  open_work: "Open work",
  unrecorded_decision: "Unrecorded decision",
  unescalated_issue: "Unescalated bug",
};

export function ChatHeader(props: ChatHeaderProps): JSX.Element {
  const { sessionKey, title, role, completion } = props;
  useEffect(() => {
    const before = document.title;
    document.title = title === null ? sessionKey : `${sessionKey} · ${title}`;
    return () => {
      document.title = before;
    };
  }, [sessionKey, title]);
  return (
    <header className="flex flex-wrap items-center gap-3 border-b px-4 py-2">
      <div data-part="heading" className="min-w-0 flex-1">
        {role === "lead" && completion != null ? (
          <Badge
            data-part="completion-badge"
            tone={completion.tone}
            title={`kev-4b verdict: ${LABEL_TEXT[completion.label]}, archive-safety ${completion.score}%`}
          >
            {completion.text}
          </Badge>
        ) : null}
        <h1
          data-part="title"
          className="m-0 truncate text-title font-semibold text-fg-strong"
        >
          {title ?? sessionKey}
        </h1>
      </div>
    </header>
  );
}
