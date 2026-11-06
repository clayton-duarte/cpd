import { useEffect } from "react";
import type { JSX } from "react";
import { Badge } from "../components/badge.tsx";
import { HoverCard, HoverCardContent, HoverCardTrigger } from "../components/hover-card.tsx";
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

function renderBadge(completion: Completion): JSX.Element {
  const others = completion.others ?? [];
  const hasCard = others.length > 0;
  const badge = (
    <Badge
      data-part="completion-badge"
      tone={completion.tone}
      {...(hasCard ? { tabIndex: 0 } : { title: `kev-4b verdict: ${LABEL_TEXT[completion.label]}, archive-safety ${completion.score}%` })}
    >
      {completion.text}
    </Badge>
  );
  if (!hasCard) return badge;
  return (
    <HoverCard openDelay={10} closeDelay={100}>
      <HoverCardTrigger asChild>{badge}</HoverCardTrigger>
      <HoverCardContent data-part="completion-scores" className="flex w-auto flex-col gap-1">
        <div className="text-xs font-semibold">kev-4b scores</div>
        {others.map((o) => (
          <Badge key={o.label} data-part="completion-other" tone="neutral">
            {`${LABEL_TEXT[o.label]} · ${o.score}%`}
          </Badge>
        ))}
      </HoverCardContent>
    </HoverCard>
  );
}

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
    <header className="flex items-center gap-3 border-b px-4 py-2">
      <div data-part="heading" className="min-w-0 flex-1">
        <h1
          data-part="title"
          className="m-0 truncate text-title font-semibold text-fg-strong"
        >
          {title ?? sessionKey}
        </h1>
      </div>
      {role === "lead" && completion != null ? renderBadge(completion) : null}
    </header>
  );
}
